# MOON / WICK / FUEL — build spec (single source of truth)

Written 2026-10-15 from: the three Workshop briefs, the live pages `/sdk/build` and `/sdk/companions`,
`BordrlessDex/bordrless-programs` @ `c6676977` and `BordrlessDex/bordrless-sdk` @ `d1a4093`.
Local copies: `ref/briefs/*.txt`, `ref/bordrless-programs/`, `ref/bordrless-sdk/`, `ref/site/`.
Rule for every agent: **read the cited source before relying on a fact here; if the source disagrees
with this file, the source wins — say so in your report.**

## 0. What we are building

Three Workshop submissions. Same engine, different constants.

| Item | Kind | Hook (SOL cut on sells) | Builder wallet slice of new pot money | Buy pace | Notes |
|---|---|---|---|---|---|
| MOON  | pair      | yes (shared code) | 20 % (`payout_bps` 2000) | 1/480 of buy pile per minute | version A |
| WICK  | pair      | yes (same code)   | 10 % (`payout_bps` 1000) | 1/240 of buy pile per minute | version B |
| FUEL | companion | none              | 10 % (`payout_bps` 1000) | only on dips (1/240 of buy pile at a 3 % drop, up to 4x at 12 %, 5-minute cooldown) | script only, hook-less coin, `dip_only` (see `script/src/dip.rs`, HANDOFF 'FUEL buys only dips') |

A/B logic: MOON vs WICK = builder slice + pace. FUEL differs from WICK in two ways now (no sell cut, and it buys only dips).

Names the owner chose: MOON, WICK. FUEL is a placeholder.

### Decided by the owner (2026-10-15)
* Names: MOON, WICK. Third item: FUEL (owner chose it).
* Builder wallet: `AxoN4hpQJWKFkSimhqnA7dUsqf5VGojA5RhhZTiP4KFm` (saved in `WALLET.txt`; set with `tools/set_wallet.py`).
* Author share: 50 %. `launcherShareBps`: 5000 on all three items. Config creator fee: 2 %.

### Open decisions (owner's; code must not block on them — they are constants / config)
1. **Builder wallet** (Solana pubkey). Unknown. Placeholder constant + `tools/set_wallet.py`.
2. **Author share at submission**: 50 % or nothing. NEW RULE (programs `b91b673`, 2026-10-15): the author
   gets 50 % of *the launcher's part of the creator fee only* — never the pot, never the hook's cut
   (`steps.rs` claim_fees, lines ~535-660). The handoff's "choose nothing, it halves the pot" is stale.
   Recommendation: take 50 %. Skipped when author == launcher (`s.author != c.beneficiary`).
3. **`launcherShareBps`** in keeper.json (0..9500): launcher's part of each trade's fee; rest = companion
   fee = pot. Same value for MOON and WICK. Live docs only (`/sdk/companions`); not in SDK 0.14.1 schema.
4. **Config creator fee** (pair config: any quarter percent 0.25–5 %). Trader pays ≈ fee × 1.25 + 0.3 % LP.
5. Third item's name.

## 1. Verified facts the design rests on (cite → file)

F1. Native SOL: every coin launched from 2026-10-15 is native (`Launch::is_native()`,
    `bordrless_launch/src/state.rs`). Pot = `Companion.pending_pot` (lamports above rent of the creator
    PDA). Swap with `launch_client::swap_v2_with_base_slice`. Scripts on `NATIVE_SCRIPT_HOLD`
    (PumpTek's) are refused for new launches — that is why Pump Tek is not on the shelf.
F2. Step accounts: `[creator(signer,w), mint, launch, companion, pool, state(w)]` then extras the keeper
    brings (`script.rs` `script_step`). Extras come from keeper.json: `swapAccounts` (swap ix accounts,
    include hook slice), `mintWritable` (live, `/sdk/companions`), `wallets` (+ the PayoutTerms account).
F3. Companion checks after the step (`script.rs`): `spent <= pending_pot` (ScriptOverspent); creator token
    balance `>= min(before, dev_bag)` (ScriptTookDevBag); lamports not below rent0 (ScriptTookLamports);
    custody intact; claims not made inside a step. The launch must be `launched`; step ≥ 60 s after launch,
    ≥ `interval` apart. `SCRIPT_BOUNDS`: interval 10–86400 s, state ≤ 10240 bytes, init ≤ 512 bytes.
F4. Return data: the script's own return data is the *answer*. Version byte 2 = `WalletPayoutAnswer`:
    `u8 version(2) | u32 n | n × { to: Pubkey(32), lamports: u64, tag: u32 }` = 1+4+44n bytes
    (`payout.rs` `decode_script_answer`). Data left by other programs (swap, token, hooks) is ignored.
    A malformed own answer fails the whole step. Empty answer → no bounty; no answer → flat bounty
    `SCRIPT_BOUNTY = 20_000` (≤ 100_000 asserted); n entries → `20_000 * min(n,8)/8`.
F5. Wallet payout checks (`payout.rs` `pay_wallets`), all before any payment, any failure fails the step:
    `terms.wallets_allowed()` (byte 100 of PayoutTerms == 1, else `WalletPayoutsOff`); entry lamports > 0;
    no duplicates; each `lamports <= holding_bps-cap` and sum `<= budget`; wallet account passed among the
    step's remaining accounts, **writable**, not executable, not sysvar/reserved, owner not a Bordrless
    program or the upgradeable loader, `lamports + pay >= rent_exempt(data_len)`, and not one of the launch's
    own accounts / the hook / the mint / default key.
F6. Budget formula (replicate EXACTLY; "an answer is refused, never clamped"; `payout.rs` `step_budget`,
    `window_base`, `budget_of`):
    ```
    window_start_now = floor(now / max(window_secs,1)) * max(window_secs,1)   // div_euclid
    (w_pot, w_paid)  = if terms.window_start == window_start_now { (terms.window_pot, terms.window_paid) }
                       else { (pot_after_script, 0) }
    window_base      = max(pot_after_script + w_paid, w_pot)
    budget           = min( bps(pot_before, step_bps),
                            bps(window_base, window_bps) - w_paid (saturating),
                            pot_after_script - bounty (saturating) )
    cap_per_entry    = bps(budget, holding_bps)
    bps(x, b)        = x * b / 10_000   (floor, u128)
    ```
    `pot_before` = `Companion.pending_pot` at step start. `pot_after_script` = pot_before − SOL the script
    spent (creator lamports delta). `bounty` = bounty for the answer's entry count; the script uses the
    upper bound 100_000 (safe: smaller budget is always within the real one).
F7. Account layouts the script must read by byte offset (the Workshop script template has NO
    `bordrless-companion` / `bordrless-swap` crate): `Companion.pending_pot` (expected offset 270, u64 LE),
    `PayoutTerms` (128 bytes, CORRECTED: step_bps@42 u16, holding_bps@44, window_bps@46, window_secs@48 u32,
    window_start@52 i64, window_pot@60 u64, window_paid@68 u64, reserved[0]@100), `Pool` reserves
    (`crates/bordrless-strategy/src/lib.rs` `pool_reserves`: base_mint@11, quote_mint@43, hook Option tag@171
    (0→+1, 1→+33), then flags u16, lp_fee_bps u16, protocol_fee_bps u16, base_reserve u64, quote_reserve u64,
    virtual_base u64, virtual_quote u64). **Every offset must be pinned by a harness test that serializes the
    real struct from the programs repo and reads it back with the script's reader.**
F8. Hook SOL cut: `quote_cut(ctx, args: PoolHookArgs) -> Result<()>`; accounts in order `[launchpad signer
    PDA(["quote-cut", hook], launchpad), launch, pool, base mint, quote mint, actor, actor's holding,
    hook state PDA(["state", mint], hook)]`; answer = `set_return_data(&u16.to_le_bytes())`; launchpad
    takes `min(answer, cap)`; creator/companion trades are exempt; Bordrless adds 25 % on top. In the After
    phase of a sell the args carry **post-swap** reserves and `amount_out` (net of the DEX's sell protocol fee)
    (`swap.rs` ~lines 686-697). Pre-sale depth = `quote_reserve + virtual_quote + amount_out`.
    The launchpad asks on every buy (answer 0). `hook.json`: `{"version":1,"quoteCut":{"maxBuyBps":1,"maxSellBps":780}}`.
F9. Custom hook flags must be non-zero (`launch_config.rs` `check_custom_hook`). The Workshop detects flags
    from source (`detectHookFlags`, `ref/bordrless-sdk/packages/shared/src/studio.ts` ~705-736): a callback is
    "on" when its whitespace-stripped body differs from the stub `check_call(&ctx,&args)?;Ok(HookReturn::default())`
    (after_* stub: `check_call(&ctx,&args)`). **Design choice: exactly one active callback, `after_burn`,
    which adds the burned amount to a counter in the hook's state** (flag AFTER_BURN = 32). No callback runs on
    buys, sells or sends, so the hook can never refuse an ordinary trade.
F10. Hook standard `prepare`: accounts payer(signer,w), mint, state PDA(["state",mint]) (init, w),
    registry PDA(["bordrless-hook-accounts", mint]) (w), system program; writes the registry with
    `write_registry`. Registry ≤ 4 extras. Ours lists exactly one: the hook state (writable), seeds
    `[Literal("state"), Account(1)]`. Starter to copy the shape from:
    `ref/bordrless-programs/programs/tests/fixtures/starters/jackpot/src/lib.rs`.
F11. Hook slice for the script's swap/burn: `launch_client::custom_hook_slice(hook, extras)` =
    `[hook (ro), token-program signer for hook (ro), extras…]`. For MOON/WICK extras = `[hook state PDA, writable]`
    where hook = `launch.custom_hook`. For FUEL (no hook) slice = `[]`.
F12. Burn: `token_client::burn_with(me, held, mint, Some(Hook{program,signer}) | None, extras, amount)`;
    needs the mint writable in the step's accounts (`mintWritable: true` in keeper.json). Burn exactly
    `holding_after − holding_before` of the swap, never more (dev bag rule). Reference implementation of
    buy+burn: `fixtures/scripts/example/src/lib.rs` `buy_burn`, test `tests/tests/companion_script_burn.rs`.
F13. Submission limits: ≤ 8 files and ≤ 120 000 bytes per half; lower-case `src/*.rs`; keeper.json ≤ 4096 B;
    hook.json ≤ 1024 B; name ≤ 32 chars; notes (builder's words) ≤ 600 chars; 6 submissions/day;
    submission fee 0.1 SOL default (`WORKSHOP_SUBMIT_LAMPORTS`), non-refundable; deploy ≈ 1 SOL per 150 kB.
F14. Source rules (checked statically on every half; one violation stops it): no `unsafe`, `asm!`,
    `macro_rules!`, `cfg_attr`, `extern`, `no_mangle`, `link`; no `include_*!`, `env!`, `option_env!`, `#[path]`;
    no `std::fs/net/process/os/env/thread`; exactly one `declare_id!` and one `#[program]`. Hook: no
    `invoke*`, no `CpiContext`, no `cpi` module, moves no SOL, no close/realloc. AI reading flags as high:
    behaviour change after launch, hard-coded address its purpose doesn't explain, text addressed to a
    reviewer/AI/auditor, unchecked arithmetic, callbacks anyone can make fail for everyone, over-sized accounts.
    Write every rate/threshold/period as a named constant; say each branch's side; document layouts with offsets.
F15. Not audited. Never write "audited", "safe", "guaranteed" about it anywhere.

## 2. Hook design (shared by MOON and WICK) — crate `hook/`

* Cargo.toml: the builder's hook template **verbatim** (`ref/briefs/hook-brief.txt` lines ~28-60).
* Files: `src/lib.rs` (+ `hook.json` beside `src/`).
* Constants (named, commented): `CUT_SLOPE_BPS: u128 = 7_800` (0.78 × the sale's share of pool depth, in bps per
  whole pool), `CUT_CAP_BPS: u16 = 780`, `BUY_CUT_BPS: u16 = 0`.
* `pub fn sell_cut_bps(amount_out: u64, quote_reserve: u64, virtual_quote: u64) -> u16`:
  `depth = quote_reserve + virtual_quote + amount_out` (u128); `0` if depth == 0 or amount_out == 0;
  `min(amount_out * CUT_SLOPE_BPS / depth, CUT_CAP_BPS)`. Share 1 % → 78 bps; share 10 % → 780 (cap).
* `quote_cut`: verify launchpad signer; buys (`direction == 1`) and any non-After sell → answer 0; After sell →
  `sell_cut_bps`. Reads only, never writes, never fails for a valid launchpad call, never panics.
* `prepare`: standard (F10). State = `{ version u8, bump u8, mint Pubkey, burned u64 }` (+8 disc = 50 bytes).
* `after_burn`: `check_call`, then `state.burned = state.burned.saturating_add(args.amount)`. Only active callback.
* `check_call`: hook signer == `hook_signer(&TOKEN_PROGRAM, &crate::ID).0` and `is_signer`; `args.mint == state.mint`.
* Pure function must be `pub` so the harness can test it.

## 3. Script design (MOON, WICK, FUEL) — crate `script/` (template) → `dist/<variant>/script/`

* Cargo.toml: the builder's companion template **verbatim** (`ref/briefs/companion-brief.txt` ~146-164).
* Files: `src/lib.rs` (program), `src/dip.rs` (the dip rule), `src/layout.rs` (foreign-account readers), `src/math.rs` (pure functions),
  `src/params.rs` (the only file that differs per variant) + `keeper.json`.
* `params.rs`: `pub struct Params { payout_bps: u16, pace_steps: u32, needs_hook: bool, min_buy: u64,
  max_buy_pool_bps: u16, min_out_bps: u16, bounty_bound: u64, dip_only: bool }`, `pub struct Dip` / `pub const DIP` (trigger 300, ref step 500, 64 moves, 4x, 300 s cooldown, step 60 s) and `pub const PARAMS: Params`.
  MOON `{2000, 480, true, 500_000, 100, 9_000, 100_000, false}`; WICK `{1000, 240, true, …same}`;
  FUEL `{1000, 240, false, …same, dip_only: true}`. (min_buy lowered from 2_000_000 to 500_000 after review: the larger floor made MOON and WICK pace identical on small pots.) Plus `pub const BUILDER_WALLET: Pubkey` (placeholder until the owner supplies one).
* Script state (script-owned, `#[account(zero)]`, **128 bytes**, keeper.json `stateLen: 128`):
  `disc[8] | version u8 =1 | mint[32] | last_pot u64 | owed u64 | paid_total u64 | burned_total u64 | steps u64 | unsplit u64 | ref_price u128 | ref_at i64 | last_buy_at i64 | reserved[7]` (`unsplit` parks new pot money from steps where payouts were off; the three dip fields stay zero unless `dip_only`; `last_pot` is the pot as the companion leaves it, after the step's payout AND the sender bounty).
  Layout comment with offsets in the source.
* `companion_init(ctx, mint, init_data)`: creator signer = PDA(["creator", mint], COMPANION); `init_data` must be empty;
  writes version, mint, zeros. Spends nothing.
* `companion_step(ctx, mint)`, in this order (all pure decisions first, then effects; any error reverts the step):
  1. Verify creator PDA + signer; `launch` via `read_launch`, `launch.mint == mint`, `launch.pool == pool`,
     `launch.is_native()` else error `NotNative`; `launch.custom_hook.is_some() == PARAMS.needs_hook` else `WrongHookSetup`;
     `launch.modules == 0` else `Unsupported`.
  2. `companion` account: owner == COMPANION_ID, mint field matches, read `pending_pot` (layout.rs).
  3. `PayoutTerms` PDA (["payout-terms", mint] under COMPANION_ID) among remaining accounts: owner == COMPANION_ID, len 128,
     mint matches; read terms + `wallets_allowed`. Missing/disabled ⇒ `payouts_on = false` (log with `msg!`, no accrual).
  4. `pool` reserves (layout.rs); `pool.owner == DEX program`; base mint == mint.
  5. `inflow = pot.saturating_sub(state.last_pot)`; if `payouts_on`: `state.owed += inflow * payout_bps / 10_000`.
     `pile = pot − owed` (saturating).
  6. Buy sizing (`math::buy_amount`): `raw = pile / pace_steps`; `amount = max(raw, min_buy)` if `pile >= min_buy`;
     `amount = min(amount, pile, quote_reserve * max_buy_pool_bps / 10_000)`; `amount < min_buy` ⇒ no buy.
  7. If buying: require the mint account writable in the step (else error `MintNotWritable`, nothing spent);
     `min_out = max(1, base_eff * amount / (quote_eff + amount) * min_out_bps / 10_000)` with
     `base_eff = base_reserve + virtual_base`, `quote_eff = quote_reserve + virtual_quote`;
     build slice (F11); CPI `swap_v2_with_base_slice(keys, me, me, 1, amount, min_out, slice)` via `invoke` over
     `ctx.accounts.to_account_infos() + remaining_accounts`; measure `spent` = creator lamports before − after;
     `bought` = creator token holding after − before; CPI `burn_with` of exactly `bought`; log nothing secret.
  8. `pot_after = pot − spent`. If `payouts_on && owed > 0 && wallet account (key == BUILDER_WALLET) present`:
     compute budget (F6) with `bounty_bound`; `pay = min(owed, budget, cap_per_entry)`; require wallet writable,
     system-owned, not executable, `lamports + pay >= rent(data_len)` else skip (keep owed); `pay == 0` ⇒ skip.
  9. If `pay > 0`: `set_return_data` of the F4 answer, 1 entry `{to: BUILDER_WALLET, lamports: pay, tag: steps as u32}`
     — **last**, after every CPI.
  10. State: `owed -= pay; last_pot = pot_after − pay; paid_total += pay; burned_total += bought; steps += 1`.
* All arithmetic checked or saturating; u128 intermediates; never panic on anything a cranker controls
  (accounts, order, signers). Extra/garbage accounts are ignored; required accounts are found **by key**.
* Errors (Anchor `#[error_code]`): `NotNative`, `WrongHookSetup`, `Unsupported`, `BadAccount`, `MintNotWritable`, `MathOverflow`.

### keeper.json (per variant; validate with a copy of `ref/bordrless-sdk/packages/shared/src/keeper.ts`)
```json
{
  "version": 1,
  "holdings": null,
  "swapAccounts": true,
  "hookAccounts": false,
  "mintWritable": true,
  "payouts": false,
  "wallets": ["<BUILDER_WALLET>"],
  "stepSecs": 60,
  "stateLen": 128,
  "payoutTerms": { "stepBps": 2000, "holdingBps": 10000, "windowBps": 5000, "windowSecs": 86400 }
}
```
MOON `stepBps 2000`; WICK/FUEL `stepBps 1000`. `launcherShareBps` added once the owner decides (decision 3).
`mintWritable`, `payoutTerms`, `launcherShareBps` are documented live (`/sdk/companions`) but absent from the
SDK 0.14.1 schema: the local validator must be extended with them; the submit page is the final judge.

## 4. Variants and files

```
moon-wick/
  hook/                       builder's hook Cargo.toml, src/lib.rs, hook.json
  script/                     builder's script Cargo.toml, src/{lib,layout,math,params}.rs, keeper.json  (MOON params)
  variants/{moon,wick,ember}/ params.rs + keeper.json (+ notes.txt: the ≤600-char builder's words)
  tools/make_variants.py      copies hook/ + script/ into dist/<variant>/, swaps params.rs/keeper.json, checks sizes/limits
  tools/set_wallet.py         validates base58 pubkey, rewrites BUILDER_WALLET in params.rs + keeper.json for all variants
  tools/scan_rules.py         static scan implementing F14 over dist/**
  harness/                    independent cargo crate: path-deps on hook + script (feature no-entrypoint) and git-deps on the
                              REAL programs crates (dev only); all tests live here
  dist/                       generated, what gets submitted
```
`hook/`, `script/` keep the builder's `[workspace] members=["."]` table, so they are independent workspace
roots; the harness depends on them by path. Share one `CARGO_TARGET_DIR=moon-wick/target`.
Pin git deps in the harness to `rev = "c6676977ddc71357c15a2af7d68e04bb095e23a2"`.

## 5. Tests (host; no Solana toolchain needed) — all in `harness/tests/`

T1 offsets: serialize real `Companion`, `PayoutTerms`, `Pool`, `WalletPayoutAnswer`, `Launch` from the programs repo with
   random field values; assert the script's readers return them (guards F7 against layout drift).
T2 budget parity: differential test of `math::step_budget` vs `bordrless_companion::instructions::step_budget`
   + `window_base` over ≥ 100 000 random inputs incl. edge cases (zero pot, window roll, huge values).
T3 answer encoding: bytes == real `WalletPayoutAnswer` Borsh; decodes with real `decode_script_answer`.
T4 cut curve: `sell_cut_bps` table (share 0.1 %, 1 %, 5 %, 10 %, 50 %, 100 %, depth 0, amount 0, u64::MAX), monotone,
   ≤ 780, never panics; launchpad's `min(answer, cap)` agreement.
T5 pacing/buy sizing: property tests (never > pile, never > 1 % pool, never < min_buy when buying, floor behaviour,
   simulated 24 h drain curve for MOON/WICK vs closed form), no-overspend invariant `spent <= pending_pot`.
T6 accounting simulation: a pure model of N steps with random inflows/bounties asserting: wallet gets ≤ payout_bps of
   inflow (+rounding), `owed` never negative, `pot_after − pay >= 0`, replicated budget never exceeded, total conserved.
T7 state layout size == 128 and offsets; init zeroed state accepted, non-zero refused (host-side logic only).
T8 variants differ only in `params.rs`/`keeper.json` (diff check) and keeper.json passes the validator.
T9 (needs SBF toolchain, owner's machine): LiteSVM integration modelled on `companion_script_burn.rs`:
   prepare → config → launch → trades → claim_fees → steps; assert supply down by burns, wallet paid, every
   companion limit, no refusal over many steps incl. window roll. Provide the test source even if it cannot run here.

## 6. Acceptance for this session

* `cargo check` + `cargo test` green for hook, script, harness on the host.
* `tools/scan_rules.py` clean on `dist/**`.
* Every F-fact used by code is covered by a test or an explicit source citation in a comment.
* A written plan (`MOON_WICK_PLAN.md`) with honest limits: nothing ran on chain; SBF build, LiteSVM and the Workshop
  trading test still to run on the owner's machine; no audit claim.


## 6. Changes after review (2026-10-16 evening)

* `dip.rs` and `dip_only` (FUEL): see HANDOFF. Buy cap follows the launch's fees (`math::pool_share_bps`, the companion's own rule, capped at `max_buy_pool_bps`).
* `last_pot` subtracts the sender bounty (`math::step_bounty`, the companion's `bounty_for`); the buy pile also leaves out the builder's share of parked (`unsplit`) money; read-only payout terms park instead of failing the step; `state.version` is checked every step; the dip reference is seeded from the launch price AND the launch time.
* T10 dip, T11 min_out, T12 bounty, T13 pins (companion id, DEX id, wallet file, on-curve wallet, pool share, reserve).
