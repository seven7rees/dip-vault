# Dip Vault - build status (2026-10-21)

Status words, as the owner asked on 2026-10-21: IMPLEMENTED / COMPILED / TEST-EXECUTED /
VALIDATED / DEPLOYED. Nothing here is VALIDATED on chain or DEPLOYED. Nothing is submitted.
Not audited.

## What exists now

| Piece | State | Evidence |
|---|---|---|
| Hook `hook_variants/dipvault/src/{lib.rs,clock.rs}` + `hook.json` | IMPLEMENTED, COMPILED (host, `no-entrypoint`) | `cd harness && cargo test` |
| Script `script_variants/dipvault/src/*.rs` (8 files) + `keeper.json` | IMPLEMENTED, COMPILED (host) | same |
| Host tests T15-T19 (40 tests) | TEST-EXECUTED, all pass | `cargo test`: 161 passed, 0 failed (T1-T14 still green) |
| Workshop rule scan (`tools/scan_rules.py dist`) | TEST-EXECUTED, clean | `python3 -I tools/make_variants.py && python3 -I tools/scan_rules.py dist` |
| SBF build (`cargo build-sbf --tools-version v1.57 --arch v3`) | COMPILED (owner approved the toolchain install 2026-10-21; solana-cli 4.3.1, platform-tools v1.57) | `target-sbf/dipvault_hook.so` 200 KB, `dipvault_script.so` 220 KB |
| T9: the real `companion_step` in LiteSVM, the whole story (`litesvm/tests/t9_dipvault.rs`) | TEST-EXECUTED, passes | `cd litesvm && cargo test` (needs the 21 Bordrless `.so` built from `ref/`, see below) |
| Workshop trading test, reading, approval | NOT DONE | needs a submission, which is the owner's step |

## T9: what the LiteSVM run proves (2026-10-21)

One test, against the Bordrless programs built from the pinned source (`ref/bordrless-programs`
@ c6676977, 15 programs + 6 test fixtures, `cargo build-sbf` each) and the pair's own SBF builds:
`prepare`; a quote-cut config (buys capped 1, sells 80) naming the hook with flags 145; a native
script launch with the payout terms and wallet payouts allowed (`companion.create`, the hook's
`prepare`, `create_script_v2`, `allow_wallet_payouts`, `launch_script`); three buys that start
hold clocks; an early sell that pays the brake (`Launch.quote_cut_accrued` grows) and a sell after
10 minutes that pays nothing; the relay in the hook state names the last seller; a real delegate sale (an old dust wallet signs the sell of a young holding's coins through
`swap_v2` with the young holding in the trader's slot): the DEX accepts it, the brake is taken
through the relay, the relay names the real source, the dust wallet's own coins are untouched;
the fees claimed into the pot; the first step splits them and pays the builder wallet; a whale pump, eight steps
that raise the reference, a dump, then a step that buys the dip and keeps the coins
(`tokens_held`); the curve filled and graduated; a step that adds locked liquidity and the LP
lands in `Launch.lp_holding` by exactly `lp_minted`; a new week in which a step measures the
three holders (touches, 0 lamports, the hook writes the round); the same holders are not touched
again that week; the next week a step pays them and each holder's hook record `paid` equals the
SOL its wallet gained. After every step the script's four piles equal the companion's
`pending_pot`.

T9 found one real bug, fixed in `c1747c1`: a due liquidity add preempted the holder payouts on
every step, so holders were never paid once the launch graduated. Now payouts take every other
step when both are due.

Measured in T9 (eight holders in the payout weeks, `cargo test -- --nocapture`):

| Step | Compute units | Instructions in the trace (limit 64) | Max CPI height (limit 5) |
|---|---|---|---|
| dip buy | 293,999 | 17 | 5 |
| liquidity add (buy, wrap, add) | 296,909 | 21 | 5 |
| measuring step, 8 touches | 415,667 | 37 | 4 |
| paying step, 8 payouts | 414,282 | 39 | 4 |

All within the keeper's budget (Bordrless cranks steps with about 600,000 CU per the SDK page).

To reproduce: install the Solana CLI (`sh -c "$(curl -sSfL https://release.anza.xyz/stable/install)"`),
build the Bordrless programs into `ref/bordrless-programs/target/deploy` (the loop in
`scripts/solana/programs.sh cmd_build`, plus the fixtures under `programs/tests/fixtures/`),
build the pair (`cargo build-sbf --manifest-path hook_variants/dipvault/Cargo.toml --sbf-out-dir
target-sbf`, the same for `script_variants/dipvault`), then `cd litesvm && cargo test`. The
`declare_id!` values in both crates are the local build keypairs' addresses (public keys only;
the keypairs stay in the ignored `target-sbf/`); Bordrless writes the real ids at submission.

## The three feasibility checks (DIP-VAULT.md BUILD ORDER 1-3): all buildable, with sources

1. Holder payouts: BUILDABLE as documented. The path is the companion program's payout answer:
   the keeper brings holdings per `keeper.json` `holdings` (a hook-data field compared with the
   current epoch), the script answers `PayoutAnswer` entries, the companion pays each owner SOL
   within the payout terms and calls the hook's `record_payout`, which writes the holding's hook
   data through the token program. Sources: `ref/bordrless-programs/docs/companions.md` "Payouts:
   a script pays holders through the companion"; `programs/bordrless_companion/src/instructions/
   payout.rs` (`pay_holdings`, every check); `programs/loss_cover_hook` + `loss_cover_script`
   (Bordrless's own pair on this path); live `bordrless.app/sdk/build` "A payout through the
   companion" (read 2026-10-21); `bordrless.app/sdk/companions` "Which holders it pays:
   keeper.json". Live proof: Loyalty Rewards (2 launches) and Lossback use it.
   Limit that shaped the design: a step pays at most 10 holdings or touches 13 (instruction
   trace), so a script cannot enumerate holders; it measures them over steps.
2. Locked liquidity: BUILDABLE, after graduation only. The DEX's `add_liquidity` refuses a
   pool still on its curve (`liquidity.rs:111 CurveLocked`). The LP minted lands in the launch's
   own LP holding (`Launch.lp_holding`), which nothing can spend: that is the lock (the
   launchpad's own first LP sits there, `docs/hooks-v2.md` §3.1). The add's quote is bridged
   SOL, so the pairing half is wrapped through the bridge first; the keeper's step carries the
   bridge accounts (`bordrless-sdk/packages/sdk/src/companion.ts` `scriptStep` appends
   `unwrapAccounts(creator)`). The companion's post-step checks allow it (`script.rs`
   `custody_intact` checks only the creator's SOL and coin holdings for delegates). Live proof:
   LiquidityTek, an approved pair (9 launches, 74 SOL added), does exactly this; its shipped
   source is in the Workshop item JSON (`ref/site/lt_now.json`) and was used as the reference
   for the instruction builders.
3. Builder scale: BUILDABLE. The script reads the pool's real SOL every step
   (`layout::read_pool_reserves`, `quote_reserve`); the hook's `quote_cut` also receives
   `PoolHookArgs.quote_reserve` (`crates/bordrless-hook/src/lib.rs:232`). The cut is applied in
   the script, where the builder's share is accrued.

Extra verification the spec asked for: `before_transfer` receives `destination_balance` and
`destination_hook_data` (`TokenHookArgs`, lib.rs:190-201), so the hold clock's fresh-coin rule is
implementable, and it is implemented. The DEX sell order (`docs/hooks-v2.md` §3.1, "A sell"):
the base transfer into the vault (which runs the token hook's `before_transfer`) comes before
`after_swap`, where the launchpad asks `quote_cut`. The relay relies on that order.

## What was built, against the approved decisions

- Buys: 0% (hook answers 0; `hook.json` maxBuyBps 1 because the launchpad always asks).
- Sell brake: 80 bps answered while the seller's hold clock is under 600 s, else 0
  (`clock::brake_bps`). The launchpad adds a quarter, so the trader pays about 1%. `hook.json`
  maxSellBps 80, and `MAX_BUY_BPS`/`MAX_SELL_BPS` constants in `lib.rs` as the live brief asks.
- Hold clock: weighted-average entry time (`clock::blended_stamp`), coins into an empty holding
  start at now, sends keep the sender's clock, emptying clears the record.
- Dust-delegate loophole: `before_transfer` writes the real source holding's clock and the slot
  into the hook state on every sell (destination owner = pool); `quote_cut` uses it when the
  slot matches, else the signer's holding. Test: T15 "a young holding sold through an old dust
  delegate still pays the brake".
- Author share 0: a submission choice, not code. `keeper.json` `launcherShareBps` 5000.
- Builder cut 1-10% by pool real SOL: `params::BUILDER_SCALE`, PROPOSED steps 1% under 5 SOL,
  2.5% from 5, 5% from 20, 7.5% from 50, 10% from 100 SOL. The owner sets the final steps.
- Pot thirds after the builder cut: 3,333 / 3,333 / 3,334 bps (`params`), kept as four piles
  in the script state that always add up to the pot (`math::Piles`, `reconcile`; T16 runs 5,000
  random steps and checks every lamport in equals every lamport out plus the pot).
- Dip buys: unchanged DIP engine (8% under the reference, 1/48 of the dip pile, at most 1% of
  the pool's real SOL, one per 5 minutes). NO BURN (owner, 2026-10-21 in chat: "no burn, lets do
  the lp thing like liquiditytek does"): the coins bought stay in the companion's holding
  (`tokens_held`), never sold, and the liquidity steps pair them into locked LP. This replaces
  DIP-VAULT.md v5 item 6's "bought tokens burned".
- Weekly holder payouts: rounds are weeks (`ROUND_SECS` 604,800, the keeper's epoch). Each
  holding the keeper brings is measured once a round (weight = balance x tier: 1x from 6 h,
  1.5x from 7 d, 2x from 30 d, plus 10% if it never sold since funded) and paid from the round's
  budget (the holder pile at the round's first step) by its share of the total measured the
  round before. The first round measures only; payouts start in week two. A holding under 6
  hours is touched (counted 0) so it is not brought again that week. At most 8 entries a step.
- Locked liquidity: at most one add per 10 minutes after graduation (yielding every other step
  to holder payouts when both are due), a tranche of 1/24 of the liquidity pile (about 4 hours a
  pile; at least 0.002 SOL, at most twice the buy cap). An add the DEX would mint no LP for is
  skipped, not attempted. The coins held (never the dev bag, read from
  `Companion.dev_tokens - dev_released`) pair with the tranche at the pool's exact ratio; only
  when they are worth less than the tranche does part of it buy more first. LP minted into
  `Launch.lp_holding`. Coins not paired stay held for the next add. `mintWritable` is false:
  nothing is ever burned.
- Step pay: the companion's own bounty (20,000 lamports flat; `SCRIPT_BOUNTY * min(n, 8) / 8`
  on a step with an n-entry answer), taken out of the piles in the order dip, lp, holders, owed.
- One action per step besides the builder payout: a dip buy if the rule fires; else a liquidity
  add if due; else holder payouts if holdings and the terms are passed. The builder wallet is
  paid, within the terms, on any step without a holder answer.

## Additive items (owner approved "additive gamification", 2026-10-21)

- `NEVER_SOLD_BONUS_BPS = 1_000`: a holding that never sold into the pool since it was funded
  weighs 10% more. One constant; 0 turns it off. Not in DIP-VAULT.md v5.
- No burn anywhere (owner decision 2026-10-21). The hook keeps `before_burn` so a holder's own
  burn still cuts their record.

## Open decisions for the owner

1. The builder scale steps (proposed above).
3. Keep the never-sold bonus at 10%, change it, or drop it.
4. Payout terms in `keeper.json`: stepBps 2000, holdingBps 10000, windowBps 10000, windowSecs
   86400. These bound the companion's checks only; the script's own piles are the real limit.
6. Quote tokens: upstream `bordrless-programs` shipped "coins launched against SPL Token or
   Token-2022 mints" on 2026-10-20 (commit d31557a, after the pin). This build assumes native SOL
   quotes (`launch.is_native()` required). A stock- or basket-quoted launch would need a new pass.

## Review findings (2026-10-21, independent pass over commits through 091dc22)

Fixed: double pay by moving coins (round-start rule); a sender omitting a holder's wallet
(skip, not touch); the liquidity buy could exceed the pool cap below the least buy (skipped);
an add the DEX would refuse (`DepositTooSmall`) blocking every later add (skipped when the LP
would be 0); the "holders waiting" check matched the pool's own vault (now the payout loop's
filter); a repeated or read-only holding failing the whole step (skipped); dust holdings in
the keeper slots (`minBalance`); LP pace 24 adds per pile; stale `dist/` (regenerated).
Open, flagged above: the registry's two fixed keys (LiquidityTek precedent, unconfirmed by the
rule text); `record_payout` as a state-writing instruction the AI reading may mark.

## Known risks and what is unverified

- The hook's `record_payout` calls `bordrless_game::write_own_hook_data`, which does a CPI into
  the token program inside the crate. The hook brief says a hook "calls no other program"; the
  pair brief's pattern 4 requires exactly this write, and Bordrless's own `loss_cover_hook`
  does it the same way. The rule scan is on the source text (no `invoke` appears in ours). The
  AI reading may still flag "an instruction besides prepare and the callbacks that writes
  state"; the code comments say why it exists. Unverified until submitted.
- The hook derives the pool at `prepare` with the launchpad's LP fee of 30 bps (LiquidityTek
  does the same). If a launch were made with another LP fee, buys and sells would not be
  recognised by the hook (every arrival still starts a clock, so the brake still works, but
  the relay would not), and the liquidity adds would be skipped by the script's own check.
- Registry extras include two fixed keys (the pool's LP mint, the launch's LP holding). The
  brief's wording allows "the hook's own and the mint's"; LiquidityTek was approved with these
  same two extras.
- The step's instruction budget: a payout step with 8 entries is within the companion's measured
  10; a liquidity step makes a swap, a wrap, an add and a burn in one step, as LiquidityTek does
  (swap + wrap + add). Not measured here.
- `keeper.json` passed `tools/scan_rules.py` only; the SDK's `checkKeeperJson` was not run (no
  node install of the SDK here).
- T9 builds the step's account list by hand from the swap and add instructions; whether
  Bordrless's keeper passes the same set (the wrap and add accounts) is unverified until a launch.
- Two design effects for the owner: every step's bounty comes out of the dip pile first; in a long
  decline the dip rule wins every step, which delays payouts and liquidity adds.
- A holder's own liquidity add (a transfer into the pool) sets its sold flag and costs the
  never-sold bonus. An established holder who tops up a large old holding barely moves its clock
  and so can sell within 10 minutes without the brake: the spec's own averaging rule.
- `keeper.json` `minBalance` is 1,000,000,000 base units (1,000 coins at the launchpad's 6
  decimals) so dust holdings cannot fill the 8 slots a step brings.
- No Workshop trading test, no mainnet.

## How to reproduce

    cd harness && cargo test            # 161 passed
    python3 -I tools/make_variants.py   # writes dist/dipvault/{hook,script}
    python3 -I tools/scan_rules.py dist # clean

Submission files: `dist/dipvault/hook/src/*.rs` + `hook.json`; `dist/dipvault/script/src/*.rs` +
`keeper.json`; words in `variants/dipvault/notes.txt` (596 chars). The owner submits.
