"""24-hour economics simulation of the MOON / WICK / FUEL script (one step a minute).

Replicates script/src/lib.rs companion_step step 5-10 and script/src/math.rs (buy_amount, step_budget),
and the companion's side of one step (ref/bordrless-programs .../instructions/script.rs and payout.rs:
bounty_for, step_budget, budget_of window roll, pot - paid - bounty). All integers, lamports. No Solana
toolchain needed. It does NOT model the DEX curve: a buy of `amount` lamports is assumed to spend exactly
`amount` of the pot (the script measures spent = creator lamports delta), so every number below is about
where the pot's SOL goes, not about the token price.

usage: python3 -I tools/sim_economics.py

Sections: 1 single deposit (1h/4h/8h/24h), 2 continuous inflow, 3 tiny pool, 4 payout-terms throttle, 5 who gets the
creator fee, 6 FUEL as written (panics), 7 sell-cut table, 8 pool too small to buy for 30 days.
"""

SOL = 1_000_000_000
MIN_BUY = 2_000_000          # params.rs min_buy
POOL_BPS = 100               # params.rs max_buy_pool_bps
BOUNTY_BOUND = 100_000       # params.rs bounty_bound (the script's upper bound)
SCRIPT_BOUNTY = 20_000       # companion constants.rs
BOUNTY_FULL = 8              # PAYOUT_BOUNTY_FULL
STEP_SECS = 60
DAY = 86_400
# 06:00 UTC on a real day (a big epoch, so the stored window_start 0 never equals the first window, as on chain)
T_START = 20_740 * DAY + 6 * 3600

VARIANTS = {                 # name: (payout_bps, pace_steps, step_bps)
    "MOON": (2_000, 480, 2_000),
    "WICK": (1_000, 240, 1_000),
    "FUEL": (1_000, 240, 1_000),   # same numbers as WICK (no hook); see finding on lib.rs:288
}
SPEC_TERMS = dict(holding=10_000, window=5_000, secs=DAY)


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
    amount = min(max(pile // max(pace, 1), MIN_BUY), pile, bps(quote_reserve, POOL_BPS))
    return None if amount < MIN_BUY else amount


class Sim:
    def __init__(self, name, pool_real, terms=None, t0=T_START, step_bps=None):
        self.payout_bps, self.pace, sb = VARIANTS[name]
        self.step_bps = sb if step_bps is None else step_bps
        self.terms = dict(SPEC_TERMS) if terms is None else dict(terms)
        self.pool = pool_real
        self.t = t0
        self.pot = self.last_pot = self.owed = 0
        self.w_start = self.w_pot = self.w_paid = 0     # companion's PayoutTerms counters
        self.inflow = self.spent = self.paid = self.bounty = 0
        self.buys = 0
        self.refusals = 0
        self.max_owed = 0
        self.log = []                                    # (step, cumulative spent, cumulative paid)

    def step(self, deposit=0):
        self.pot += deposit
        self.inflow += deposit
        pot_before = self.pot
        # script: accrue, size, buy
        self.owed += bps(max(pot_before - self.last_pot, 0), self.payout_bps)
        pile = max(pot_before - self.owed, 0)
        amt = buy_amount(pile, self.pace, self.pool)
        spent = amt or 0
        if amt:
            self.buys += 1
        pot_after = pot_before - spent
        # script: payout candidate (terms read as stored; window rolled locally)
        ws = self.t // self.terms["secs"] * self.terms["secs"]
        w_pot, w_paid = (self.w_pot, self.w_paid) if self.w_start == ws else (pot_after, 0)
        pay = 0
        if self.owed > 0:
            budget = step_budget(pot_before, pot_after, BOUNTY_BOUND, w_pot, w_paid,
                                 self.step_bps, self.terms["window"])
            cand = min(self.owed, budget, bps(budget, self.terms["holding"]))
            if cand > 0:
                pay = cand        # builder wallet assumed present, writable, rent-funded
        # companion: bounty by answer size, real budget check (a refusal would revert the whole step)
        wanted = SCRIPT_BOUNTY * 1 // BOUNTY_FULL if pay else SCRIPT_BOUNTY   # one entry: 2_500; no answer: 20_000
        bounty = wanted if pot_after >= wanted else 0
        if pay:
            real = step_budget(pot_before, pot_after, bounty, w_pot, w_paid, self.step_bps, self.terms["window"])
            if pay > real or pay > bps(real, self.terms["holding"]):
                self.refusals += 1
            self.w_start, self.w_pot, self.w_paid = ws, w_pot, w_paid + pay
        self.pot = pot_after - pay - bounty
        self.last_pot = pot_after - pay
        self.owed -= pay
        self.spent += spent
        self.paid += pay
        self.bounty += bounty
        self.max_owed = max(self.max_owed, self.owed)
        self.t += STEP_SECS

    def run(self, minutes, deposits=None):
        """deposits: function minute -> lamports added just before that minute's step."""
        marks = {}
        for m in range(minutes):
            self.step(deposits(m) if deposits else 0)
            if m + 1 in (60, 240, 480, 720, 1440):
                marks[m + 1] = (self.spent, self.paid, self.bounty, self.pot, self.owed)
        return marks


def pct(x, d):
    return f"{100 * x / d:5.1f}%" if d else "  n/a "


def sol(x):
    return f"{x / SOL:9.4f}"


def single_deposit():
    print("== 1. One deposit D into an empty pot at minute 0, pool 50 SOL (1% cap 0.5 SOL), SPEC terms, 24 h of steps")
    print("   share of D spent on buys after 1h / 8h / 24h; builder paid; cranker bounty; left in pot at 24h")
    print("   'ideal' = the listing's picture: (1 - payout) x (1 - (1 - 1/pace)^n)")
    print("variant      D(SOL)   1h     4h     8h     24h    ideal1h ideal4h ideal8h ideal24h  builder  bounty  left(pot)  min-buy-regime")
    for name in VARIANTS:
        pb, pace, _ = VARIANTS[name]
        for d_sol in (0.05, 0.2, 0.5, 1, 5, 20, 100):
            d = int(d_sol * SOL)
            s = Sim(name, 50 * SOL)
            marks = s.run(1440, lambda m, d=d: d if m == 0 else 0)
            ideal = lambda n: (1 - pb / 10_000) * (1 - (1 - 1 / pace) ** n)
            floor_regime = (d * (10_000 - pb) // 10_000) // pace < MIN_BUY
            print(f"{name}  {d_sol:8.2f}  {pct(marks[60][0], d)} {pct(marks[240][0], d)} {pct(marks[480][0], d)} {pct(marks[1440][0], d)}"
                  f"   {100 * ideal(60):5.1f}%  {100 * ideal(240):5.1f}%  {100 * ideal(480):5.1f}%  {100 * ideal(1440):5.1f}%"
                  f"  {pct(s.paid, d)} {pct(s.bounty, d)} {sol(s.pot)}  {'FLOOR (0.002 SOL a minute)' if floor_regime else 'proportional'}")
        print()


def single_deposit_floor_time():
    print("== 1b. Minutes until the pile is gone when the pile is under pace x 0.002 SOL (MOON 0.96 SOL, WICK/FUEL 0.48 SOL)")
    for name in VARIANTS:
        pb, pace, _ = VARIANTS[name]
        for d_sol in (0.05, 0.2, 0.5):
            d = int(d_sol * SOL)
            s = Sim(name, 50 * SOL)
            s.step(d)
            n = 1
            while s.buys and n < 1440 and max(s.pot - s.owed, 0) >= MIN_BUY:
                s.step()
                n += 1
            print(f"   {name} D={d_sol} SOL: last buy at minute {n}, pile left {sol(max(s.pot - s.owed, 0))} SOL (under 0.002: not buyable, and the crank bounty keeps eating it)")
    print()


def continuous(label, per_day_sol, pool_sol, claim_every=10, terms=None, step_bps=None, names=VARIANTS):
    per_claim = int(per_day_sol * SOL * claim_every / 1440)
    rows = []
    for name in names:
        s = Sim(name, int(pool_sol * SOL), terms=terms, step_bps=step_bps)
        s.run(1440, lambda m: per_claim if m % claim_every == 0 else 0)
        rows.append((name, s))
        print(f"{label:<26} {name}  in {sol(s.inflow)}  buys {sol(s.spent)} ({pct(s.spent, s.inflow)})  builder {sol(s.paid)} ({pct(s.paid, s.inflow)})"
              f"  bounty {sol(s.bounty)} ({pct(s.bounty, s.inflow)})  pot left {sol(s.pot)}  owed {sol(s.owed)}  buys# {s.buys:4d}  refused {s.refusals}")
    return rows


def fuel_as_built():
    print("== 6. FUEL as the code is written (script/src/lib.rs:288 `launch.custom_hook.expect(..)` with needs_hook = false)")
    print("   A step that has a buy to make panics and reverts whole: no buy, no burn, no builder pay, no bounty, no state update.")
    for per_day in (0.1, 1, 5):
        pot, steps_ok, steps_reverted = 0, 0, 0
        per_claim = int(per_day * SOL * 10 / 1440)
        for m in range(1440):
            if m % 10 == 0:
                pot += per_claim
            pile = pot  # last_pot / owed never move, so owed accrues nothing that survives
            if buy_amount(pile, 240, 50 * SOL) is not None:
                steps_reverted += 1      # panic -> revert
            else:
                steps_ok += 1
                pot = max(pot - SCRIPT_BOUNTY, 0)   # only steps that succeed pay the crank bounty
        print(f"   inflow {per_day:>4} SOL/day: burned 0, builder ~0 (only a step with nothing to buy can succeed), pot after 24 h {sol(pot)} SOL, reverted steps {steps_reverted}, ok steps {steps_ok}")
    print()


def cut_table():
    print("== 7. The sell cut: the listing's number vs what the seller loses (hook answer x 1.25: docs/quote-cut.md, swap.rs 'held back ... the trader pays it')")
    print("   share of pool depth (real+virtual SOL, post-sale) -> hook cut -> seller pays cut x 1.25 -> pot gets cut (builder slice of it: 20% MOON / 10% WICK)")
    slope, cap = 7_800, 780
    for share_bps in (10, 100, 250, 500, 1_000, 2_500, 5_000):
        amount, depth = share_bps, 10_000          # amount_out / depth = share
        cut = min(amount * slope // depth, cap)
        print(f"   sale = {share_bps / 100:5.2f}% of depth: cut {cut / 100:5.2f}%  seller pays {cut * 1.25 / 100:5.2f}%  (plus the normal creator fee x1.25 and LP fee on every sale)")
    print("   pre-graduation the depth includes ~28 SOL of VIRTUAL quote (core lib.rs opening(): virtual_quote 28.125 SOL, real 0):")
    for real in (1, 5, 20):
        v = 28.125
        for sale in (real * 0.1,):
            depth = real - 0 + v                    # post-sale real + virtual + sale = pre-sale real + virtual
            cut = min(sale * slope / depth, cap) / 100
            print(f"   real SOL {real:>3}, sale of {sale:.2f} SOL (10% of the real SOL): cut {cut:4.2f}%  (the listing's picture says 7.8%)")
    print()


def dead_pool_bleed():
    print("== 8. Pool too small to buy (1% cap < 0.002 SOL): one 1 SOL deposit, then nothing, 30 days (MOON/WICK)")
    for name in ("MOON", "WICK"):
        s = Sim(name, int(0.1 * SOL))
        s.run(30 * 1440, lambda m: SOL if m == 0 else 0)
        print(f"   {name}: burned {sol(s.spent)}  builder {sol(s.paid)}  cranker bounty {sol(s.bounty)}  still in pot {sol(s.pot)}  owed {sol(s.owed)}")
    print()


def main():
    single_deposit()
    single_deposit_floor_time()

    print("== 2. Continuous inflow over 24 h from an empty pot (claims every 10 min), pool 50 SOL, SPEC terms")
    for per_day in (0.1, 0.3, 1, 5, 25):
        continuous(f"inflow {per_day} SOL/day", per_day, 50)
        print()

    print("== 3. Tiny pool: 1% cap vs 0.002 SOL floor (inflow 1 SOL/day, claims every 10 min)")
    for pool in (0.05, 0.15, 0.19, 0.2, 0.25, 1, 5):
        continuous(f"pool {pool} SOL (cap {pool / 100:.4f})", 1, pool, names=("MOON", "WICK"))
        print()

    print("== 4. Terms throttle. Steady inflow 5 SOL/day, pool 50 SOL")
    cases = [
        ("SPEC terms 20%/10%, 50%/day", None, None),
        ("site-default style 10%/25%", dict(holding=10_000, window=2_500, secs=DAY), 1_000),
        ("launcher tightens 1%/1%", dict(holding=10_000, window=100, secs=DAY), 100),
    ]
    for label, terms, sb in cases:
        continuous(label, 5, 50, terms=terms, step_bps=sb, names=("MOON", "WICK"))
    print()
    print("   same, but one lump a day (a 5 SOL claim at minute 0 and nothing else), 24 h:")
    for label, terms, sb in cases:
        for name in ("MOON", "WICK"):
            s = Sim(name, 50 * SOL, terms=terms, step_bps=sb)
            s.run(1440, lambda m: 5 * SOL if m == 0 else 0)
            owed_at_start = bps(5 * SOL, VARIANTS[name][0])
            print(f"   {label:<30} {name} builder owed at deposit {sol(owed_at_start)}  paid in 24h {sol(s.paid)}  still owed {sol(s.owed)}  refused {s.refusals}")
    print()
    print("   steady 5 SOL/day for 72 h under the tightened terms (backlog that never reaches the buy pile):")
    for name in ("MOON", "WICK"):
        s = Sim(name, 50 * SOL, terms=dict(holding=10_000, window=100, secs=DAY), step_bps=100)
        per = int(5 * SOL * 10 / 1440)
        s.run(3 * 1440, lambda m: per if m % 10 == 0 else 0)
        print(f"   {name}: inflow {sol(s.inflow)} buys {sol(s.spent)} builder {sol(s.paid)} owed {sol(s.owed)} pot {sol(s.pot)}")
    print()

    print("== 4b. Builder income by day under the SPEC terms (window 50%/day) vs windowBps 100%, claims every 10 min, pool 50 SOL")
    for per_day in (0.3, 1, 2, 5):
        for label, terms in (("window 50%", None), ("window 100%", dict(holding=10_000, window=10_000, secs=DAY))):
            for name in ("MOON", "WICK"):
                s = Sim(name, 50 * SOL, terms=terms)
                per = int(per_day * SOL * 10 / 1440)
                by_day, last = [], (0, 0)
                for _ in range(7):
                    s.run(1440, lambda m: per if m % 10 == 0 else 0)
                    by_day.append(100 * (s.paid - last[0]) / (s.inflow - last[1]))
                    last = (s.paid, s.inflow)
                print(f"   {per_day:>4} SOL/day {label:<11} {name}: builder % of inflow, days 1-7: " + " ".join(f"{x:4.1f}" for x in by_day)
                      + f"   backlog still owed {sol(s.owed)} SOL")
    print()

    print("== 4c. Does the pace (480 vs 240) matter? Buys over 24 h, claims every 10 min, pool 500 SOL (cap never binds)")
    for per_day in (0.3, 1, 2, 3.5, 5, 10, 25):
        cells = []
        for name, pace in (("MOON", 480), ("MOON", 240), ("WICK", 240), ("WICK", 480)):
            VARIANTS["T"] = (VARIANTS[name][0], pace, VARIANTS[name][2])
            s = Sim("T", 500 * SOL)
            per = int(per_day * SOL * 10 / 1440)
            s.run(1440, lambda m: per if m % 10 == 0 else 0)
            cells.append(f"{name}/{pace}: {sol(s.spent)}")
        print(f"   {per_day:>4} SOL/day  " + "  ".join(cells))
    del VARIANTS["T"]
    print("   (below about 3.5 SOL/day the 480-vs-240 rows are identical: the 0.002 SOL floor sets the pace)")
    print()

    print("== 5. Where one lamport of CREATOR FEE goes (launcherShareBps 5000, claim bounty 50 bps, AUTHOR_SHARE 50% of launcher part)")
    for name in ("MOON", "WICK"):
        pb = VARIANTS[name][0]
        claim_bounty = 0.005
        to_pot = 0.5 * (1 - claim_bounty)
        author = 0.5 * 0.5 * (1 - claim_bounty)
        launcher = 0.5 * 0.5 * (1 - claim_bounty)
        builder_slice = to_pot * pb / 10_000
        buy_burn = to_pot - builder_slice
        print(f"   {name}: builder {100 * (author + builder_slice):.1f}% (author share {100 * author:.1f}% + pot slice {100 * builder_slice:.1f}%)"
              f", launcher {100 * launcher:.1f}%, claim cranker {100 * claim_bounty:.1f}%, buy-and-burn pile {100 * buy_burn:.1f}%")
    print("   (the hook's sell cut goes whole to the pot: builder slice of it = payout_bps; no author share)")
    print()
    fuel_as_built()
    cut_table()
    dead_pool_bleed()


if __name__ == "__main__":
    main()
