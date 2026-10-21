"""Does it do what the listing says? 24 h and 30 day money simulation of MOON / WICK / FUEL.

Integer lamports, one step a minute. Ports script/src/math.rs and script/src/lib.rs steps 5-10, and the
companion's side of one step (ref/bordrless-programs/.../instructions/script.rs `process_script_step`,
payout.rs `bounty_for` / `step_budget` / `budget_of`) and of `claim_fees` (steps.rs: 0.5% claim bounty first,
the hook's cut whole to the pot, the rest split by launcherShareBps 5000 so half of the rest to the pot).

Not modelled: the DEX curve (a buy of A lamports is taken to spend exactly A of the pot, as the script measures
it), price, and whether Bordrless's keeper really claims every CLAIM_EVERY minutes (unknown, a parameter).
The creator's own buys pay the creator fee too (core `creator_and_holder_fees` has no creator exemption), so
2% of each script buy goes back into the fee vault and half of it returns to the pot at the next claim.

usage: python3 -I tools/sim_listing_check.py
"""
import math
import random

SOL = 1_000_000_000
DAY_MIN = 1440
CLAIM_BOUNTY_BPS = 50           # COMPANION_DEFAULTS.bountyBps (sdk companion.ts:151)
CREATOR_FEE_BPS = 200           # owner: config creator fee 2%
POT_SHARE_BPS = 5_000           # launcherShareBps 5000 -> the rest is the pot
SCRIPT_BOUNTY = 20_000
BOUNTY_FULL = 8
BOUNTY_BOUND = 100_000
MIN_BUY = 500_000
POOL_BPS = 100
WINDOW_SECS = 86_400
HOLDING_BPS = 10_000
WINDOW_BPS = 5_000

VARIANTS = {  # name: (payout_bps, pace, step_bps, hook)
    "MOON": (2_000, 480, 2_000, True),
    "WICK": (1_000, 240, 1_000, True),
    "FUEL": (1_000, 240, 1_000, False),
    # controls for section G: same builder slice, only the pace differs
    "PACE480": (2_000, 480, 2_000, True),
    "PACE240": (2_000, 240, 2_000, True),
}


def bps(x, b):
    return x * b // 10_000


def window_base(pot_after, w_pot, w_paid):
    return max(pot_after + w_paid, w_pot)


def step_budget(pot_before, pot_after, bounty, w_pot, w_paid, step_bps, window_bps):
    a = bps(pot_before, step_bps)
    b = max(bps(window_base(pot_after, w_pot, w_paid), window_bps) - w_paid, 0)
    c = max(pot_after - bounty, 0)
    return min(a, b, c)


def buy_amount(pile, pace, quote_reserve):
    if pile < MIN_BUY:
        return None
    amt = min(max(pile // max(pace, 1), MIN_BUY), pile, bps(quote_reserve, POOL_BPS))
    return None if amt < MIN_BUY else amt


def sell_cut_bps(amount_out, quote_reserve, virtual_quote):
    if amount_out == 0:
        return 0
    depth = quote_reserve + virtual_quote + amount_out
    return min(amount_out * 7_800 // depth, 780)


class Coin:
    def __init__(self, name, real_sol, virtual_sol=0.0, step_bps=None, terms_ok=True):
        self.name = name
        self.payout_bps, self.pace, sb, self.hook = VARIANTS[name]
        self.step_bps = sb if step_bps is None else step_bps
        self.real = int(real_sol * SOL)
        self.virt = int(virtual_sol * SOL)
        self.pot = self.last_pot = self.owed = self.unsplit = 0
        self.w_start = self.w_pot = self.w_paid = 0
        self.vault_fee = 0      # creator fee waiting in the launch vault (not the hook's cut)
        self.vault_cut = 0      # the hook's cut waiting in the vault
        self.t = 20_740 * 86_400 + 6 * 3600
        self.terms_ok = terms_ok
        self.tot = dict(fee_in=0, cut_in=0, recycled=0, claim_bounty=0, step_bounty=0, builder=0, spent=0,
                        buys=0, refusals=0, idle_steps=0, bounty_when_no_buy=0)
        self.max_owed = 0

    # ---- trading side -------------------------------------------------------------------------------
    def trade(self, kind, sol):
        """kind 'buy' or 'sell'; sol = lamports in (buy) or out (sell)."""
        fee = -(-sol * CREATOR_FEE_BPS // 10_000)      # fee_amount rounds up
        self.vault_fee += fee
        self.tot["fee_in"] += fee
        if kind == "sell" and self.hook:
            cut = sol * sell_cut_bps(sol, self.real, self.virt) // 10_000
            self.vault_cut += cut
            self.tot["cut_in"] += cut

    # ---- claim_fees ---------------------------------------------------------------------------------
    def claim(self):
        got = self.vault_fee + self.vault_cut
        if got == 0:
            return
        hook_gross = self.vault_cut
        self.vault_fee = self.vault_cut = 0
        bounty = bps(got, CLAIM_BOUNTY_BPS)
        rest = got - bounty
        from_hook = rest * hook_gross // got if got else 0
        base = rest - from_hook
        to_pot = base - bps(base, 10_000 - POT_SHARE_BPS)
        self.pot += to_pot + from_hook
        self.tot["claim_bounty"] += bounty

    # ---- one script step + the companion's checks ------------------------------------------------------
    def step(self):
        pot_before = self.pot
        new_money = max(pot_before - self.last_pot, 0)
        if self.terms_ok:
            self.owed += bps(new_money + self.unsplit, self.payout_bps)
            self.unsplit = 0
        else:
            self.unsplit += new_money
        pile = max(pot_before - self.owed, 0)
        amt = buy_amount(pile, self.pace, self.real)
        spent = amt or 0
        if amt:
            self.tot["buys"] += 1
            if amt == MIN_BUY:
                self.tot["floor_buys"] = self.tot.get("floor_buys", 0) + 1
                self.tot["floor_spent"] = self.tot.get("floor_spent", 0) + amt
            if amt == bps(self.real, POOL_BPS) and amt > MIN_BUY and amt < pile // max(self.pace, 1):
                self.tot["cap_buys"] = self.tot.get("cap_buys", 0) + 1
            self.vault_fee += -(-amt * CREATOR_FEE_BPS // 10_000)     # recycled creator fee
            self.tot["recycled"] += -(-amt * CREATOR_FEE_BPS // 10_000)
        pot_after = pot_before - spent
        ws = self.t // WINDOW_SECS * WINDOW_SECS
        w_pot, w_paid = (self.w_pot, self.w_paid) if self.w_start == ws else (pot_after, 0)
        pay = 0
        if self.terms_ok and self.owed > 0:
            budget = step_budget(pot_before, pot_after, BOUNTY_BOUND, w_pot, w_paid, self.step_bps, WINDOW_BPS)
            cand = min(self.owed, budget, bps(budget, HOLDING_BPS))
            if cand >= 890_880 or True:      # wallet assumed rent-funded
                pay = cand
        wanted = SCRIPT_BOUNTY * 1 // BOUNTY_FULL if pay else SCRIPT_BOUNTY
        bounty = wanted if pot_after >= wanted else 0
        if pay:
            real = step_budget(pot_before, pot_after, bounty, w_pot, w_paid, self.step_bps, WINDOW_BPS)
            if pay > real:
                self.tot["refusals"] += 1
            self.w_start, self.w_pot, self.w_paid = ws, w_pot, w_paid + pay
        self.pot = pot_after - pay - bounty
        self.last_pot = pot_after - pay
        self.owed -= pay
        self.tot["spent"] += spent
        self.tot["builder"] += pay
        self.tot["step_bounty"] += bounty
        if not spent:
            self.tot["idle_steps"] += 1
            self.tot["bounty_when_no_buy"] += bounty
        self.max_owed = max(self.max_owed, self.owed)
        self.t += 60
        return spent


def run_flow(name, minutes, volume_sol_day, real_sol, virtual_sol=0.0, claim_every=30, seed=7,
             median_sol=0.2, whale_frac=0.02, whale_pool_share=(0.01, 0.08)):
    rng = random.Random(seed)
    c = Coin(name, real_sol, virtual_sol)
    depth = real_sol + virtual_sol
    mean_retail = median_sol * math.exp(0.5)            # sigma 1.0
    mean_whale = depth * sum(whale_pool_share) / 2
    mean_trade = (1 - whale_frac) * mean_retail + whale_frac * mean_whale
    lam = volume_sol_day / DAY_MIN / mean_trade            # trades per minute
    sell_vol = 0
    cut_paid = 0
    for m in range(minutes):
        n = 0
        L = math.exp(-lam)
        p, k = 1.0, 0
        if lam < 30:
            while True:
                p *= rng.random()
                if p <= L:
                    break
                k += 1
            n = k
        else:
            n = max(0, int(rng.gauss(lam, math.sqrt(lam))))
        for _ in range(n):
            if rng.random() < whale_frac:
                sz = depth * rng.uniform(*whale_pool_share)
            else:
                sz = median_sol * math.exp(rng.gauss(0, 1.0))
            sol = int(sz * SOL)
            kind = "buy" if rng.random() < 0.5 else "sell"
            if kind == "sell":
                sell_vol += sol
            c.trade(kind, sol)
        if (m + 1) % claim_every == 0:
            c.claim()
        c.step()
    return c, sell_vol


def fmt(x):
    return f"{x / SOL:11.4f}"


def pct(x, d):
    return f"{100 * x / d:6.1f}%" if d else "   n/a "


# -------------------------------------------------------------------------------------------------------
def section_cut():
    print("== A. Sell cut: the listing says 0.78% per 1% of 'the pool', cap 7.8%, x1.25 -> 9.75%")
    print("   depth used by the hook = real quote + VIRTUAL quote + amount_out (hook lib.rs sell_cut_bps)")
    print("   a sale of 2 SOL on a curve coin, real 10 SOL, virtual 30 SOL; share of real vs share of depth")
    for real, virt in ((10, 30), (10, 0), (1, 30), (100, 30)):
        out = 2 * SOL
        bpsv = sell_cut_bps(out, real * SOL, virt * SOL)
        share_real = 2 / real
        share_depth = 2 / (real + virt + 2)
        naive = min(0.78 * share_real * 100, 7.8)
        print(f"   real {real:>4} virt {virt:>3}: sale 2 SOL = {share_real*100:6.1f}% of the real SOL, {share_depth*100:5.1f}% of depth"
              f" -> cut {bpsv/100:5.2f}% (a reader of 'of the pool' = real SOL expects {naive:4.2f}%), seller pays {bpsv*1.25/100:5.2f}%")
    print()


def section_single_deposit():
    print("== B. One deposit D into an empty pot at minute 0, then 24 h of steps, no other inflow")
    print("   listing: 'At 1/480 a minute about 63% of a deposit is spent after 8 hours and about 95% after 24. At 1/240, 63% after 4 hours and 95% after 12.'")
    print("   shown: share of D spent on buys; builder share; step bounties; left in pot. ideal = (1-payout) x (1-(1-1/pace)^n)")
    print("name  poolreal    D(SOL)     8h      24h   | 4h(240)  12h(240) | builder  bounty  left | regime")
    for name in ("MOON", "WICK", "FUEL"):
        pb, pace, _, _ = VARIANTS[name]
        for real in (5, 50, 500):
            for d in (0.05, 0.2, 0.5, 2, 20, 200):
                c = Coin(name, real)
                c.pot = int(d * SOL)
                marks = {}
                for m in range(DAY_MIN):
                    c.step()
                    if m + 1 in (240, 480, 720, 1440):
                        marks[m + 1] = c.tot["spent"]
                dd = int(d * SOL)
                floor = (dd * (10_000 - pb) // 10_000) // pace < MIN_BUY
                capb = (dd * (10_000 - pb) // 10_000) // pace > bps(int(real * SOL), POOL_BPS)
                regime = "MIN-BUY FLOOR (faster)" if floor else ("POOL CAP (slower)" if capb else "1/pace")
                ideal8 = (1 - pb / 10_000) * (1 - (1 - 1 / pace) ** 480)
                print(f"{name} {real:>6} {d:>10.2f} {pct(marks[480], dd)} {pct(marks[1440], dd)} | {pct(marks[240], dd)} {pct(marks[720], dd)} |"
                      f" {pct(c.tot['builder'], dd)} {pct(c.tot['step_bounty'], dd)} {pct(c.pot, dd)} | {regime}")
        print()


def section_flows(minutes, label, scen):
    print(f"== C. {label}: continuous trading; claim every 30 min; MOON/WICK have the sell cut, FUEL does not")
    print("   columns: SOL into pot from fees / hook cut / recycled script-buy fee; claim bounty; STEP BOUNTY; builder paid; spent on buys (burn);")
    print("   then the shares of everything that reached the pot. steps with no buy and what they paid in bounty.")
    for volume, real, virt, note in scen:
        print(f"-- volume {volume} SOL/day, pool real {real} SOL, virtual {virt} SOL  ({note})")
        print("name   fee-in    cut-in   recycled  clmbounty  stepbounty   builder    burned-SOL | bounty%  builder%  burned%  left%  | idleSteps  avgcut(sell)")
        for name in ("MOON", "WICK", "FUEL"):
            c, sell_vol = run_flow(name, minutes, volume, real, virt)
            t = c.tot
            reached = t["fee_in"] // 2 + t["cut_in"] + t["recycled"] // 2
            into_pot = t["spent"] + t["builder"] + t["step_bounty"] + c.pot
            avgcut = t["cut_in"] / sell_vol * 100 if sell_vol else 0
            print(f"{name} {fmt(t['fee_in'])} {fmt(t['cut_in'])} {fmt(t['recycled'])} {fmt(t['claim_bounty'])} {fmt(t['step_bounty'])} {fmt(t['builder'])} {fmt(t['spent'])}"
                  f" | {pct(t['step_bounty'], into_pot)} {pct(t['builder'], into_pot)} {pct(t['spent'], into_pot)} {pct(c.pot, into_pot)}"
                  f"  | {t['idle_steps']:>6}/{minutes}  {avgcut:5.2f}%   refusals={t['refusals']} maxOwed={c.max_owed/SOL:.4f}")
        print()


def section_builder_cap():
    print("== D. Builder's share of NEW pot money: is it ever above payout_bps of what flowed in? (30 days, several scenarios)")
    worst = {}
    for name in VARIANTS:
        pb = VARIANTS[name][0]
        for volume, real, virt in ((2, 5, 30), (40, 50, 0), (400, 200, 0), (6000, 1000, 0)):
            c, _ = run_flow(name, 30 * DAY_MIN, volume, real, virt, seed=3)
            t = c.tot
            inflow = t["fee_in"] // 2 + t["cut_in"] + t["recycled"] // 2
            # pot income as the script sees it = what it spent + paid + bounties + left (the pot is closed)
            seen = t["spent"] + t["builder"] + t["step_bounty"] + c.pot
            share = t["builder"] / seen if seen else 0
            worst[name] = max(worst.get(name, 0), share)
            print(f"   {name} vol {volume:>5} real {real:>5}: builder {t['builder']/SOL:9.4f} of pot flow {seen/SOL:9.4f}  = {100*share:5.2f}% (limit {pb/100:.0f}%)  owed left {c.owed/SOL:.4f}")
    print("   worst:", {k: f"{100*v:.2f}%" for k, v in worst.items()})
    print()


def section_floor_regime():
    print("== F. Which regime do the notes' words ('spends 1/480 of the rest') describe? Share of buys that are the flat 0.0005 SOL floor, 30 days")
    print("   (steady-state pile is about pace x per-minute pot inflow; below 0.24 SOL for MOON / 0.12 SOL for WICK/FUEL every buy is the floor)")
    for volume, real, virt in ((3, 10, 30), (20, 30, 30), (50, 30, 30), (100, 100, 0), (500, 100, 0), (5000, 500, 0)):
        row = []
        for name in ("MOON", "WICK", "FUEL"):
            c, _ = run_flow(name, 30 * DAY_MIN, volume, real, virt, seed=5)
            t = c.tot
            b, f = t["buys"], t.get("floor_buys", 0)
            row.append(f"{name}: {b:>6} buys, {100*f/b if b else 0:5.1f}% at floor ({100*t.get('floor_spent',0)/t['spent'] if t['spent'] else 0:5.1f}% of burned SOL)")
        print(f"   vol {volume:>5} SOL/day real {real:>4}: " + " | ".join(row))
    print()


def section_pace_only():
    print("== G. Does the pace (1/480 vs 1/240) alone make a faster burn? Same 20% builder slice, only pace differs, 30 days")
    for volume, real, virt in ((3, 10, 30), (20, 30, 30), (50, 30, 30), (100, 100, 0), (200, 100, 0), (500, 100, 0), (5000, 500, 0)):
        a, _ = run_flow("PACE480", 30 * DAY_MIN, volume, real, virt, seed=5)
        b, _ = run_flow("PACE240", 30 * DAY_MIN, volume, real, virt, seed=5)
        sa, sb = a.tot["spent"], b.tot["spent"]
        print(f"   vol {volume:>5} SOL/day real {real:>4}: pace 1/480 burned {sa/SOL:10.3f} SOL, pace 1/240 burned {sb/SOL:10.3f} SOL  -> 1/240 is {100*(sb-sa)/sa if sa else 0:5.1f}% more;"
              f" pot left {a.pot/SOL:8.3f} vs {b.pot/SOL:8.3f}")
    print()


def section_bounty_floor():
    print("== E. The idle-step bounty (20,000 lamports/min = 0.0288 SOL/day) against small inflows (no builder terms trouble, pool 50 SOL)")
    print("   daily volume V SOL -> fee pot inflow/day (1% of V) vs bounty/day; share of pot inflow that goes to the cranker; buys in 30 days")
    for v in (0.5, 1, 2, 3, 5, 10, 25, 100):
        c, _ = run_flow("WICK", 30 * DAY_MIN, v, 50, 0.0, seed=11)
        t = c.tot
        into = t["spent"] + t["builder"] + t["step_bounty"] + c.pot
        print(f"   V={v:>6} SOL/day: pot in ~{v*0.01:7.4f} SOL/day, bounty paid {t['step_bounty']/SOL/30:7.4f} SOL/day -> bounty {pct(t['step_bounty'], into)} of the pot,"
              f" burned {pct(t['spent'], into)}, builder {pct(t['builder'], into)}, buys {t['buys']}")
    print()


if __name__ == "__main__":
    section_cut()
    section_single_deposit()
    section_flows(DAY_MIN, "24 hours", [
        (3, 10, 30, "tiny curve coin"),
        (50, 30, 30, "small curve coin"),
        (500, 100, 0, "graduated, active"),
        (5000, 500, 0, "graduated, viral"),
    ])
    section_flows(30 * DAY_MIN, "30 days", [
        (3, 10, 30, "tiny curve coin"),
        (50, 30, 30, "small curve coin"),
        (500, 100, 0, "graduated, active"),
        (5000, 500, 0, "graduated, viral"),
    ])
    section_builder_cap()
    section_floor_regime()
    section_pace_only()
    section_bounty_floor()
