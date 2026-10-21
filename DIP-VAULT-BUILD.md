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
| SBF build (`cargo build-sbf`) | NOT DONE | no Solana toolchain on this machine; the owner asked to be asked first |
| T9-style LiteSVM run of the real `companion_step` | NOT DONE | needs the SBF build above |
| Workshop trading test, reading, approval | NOT DONE | needs a submission, which is the owner's step |

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
  the pool's real SOL, one per 5 minutes), bought coins burned (`BURN_DIP_BUYS = true`; one
  constant to flip, see open decisions).
- Weekly holder payouts: rounds are weeks (`ROUND_SECS` 604,800, the keeper's epoch). Each
  holding the keeper brings is measured once a round (weight = balance x tier: 1x from 6 h,
  1.5x from 7 d, 2x from 30 d, plus 10% if it never sold since funded) and paid from the round's
  budget (the holder pile at the round's first step) by its share of the total measured the
  round before. The first round measures only; payouts start in week two. A holding under 6
  hours is touched (counted 0) so it is not brought again that week. At most 8 entries a step.
- Locked liquidity: every other step after graduation, a tranche of 1/48 of the liquidity pile
  (at least 0.002 SOL, at most twice the buy cap) buys the coin with 48.5% and pairs the coins at
  the pool's exact ratio with the rest; LP minted into `Launch.lp_holding`; leftover coins burned.
- Step pay: the companion's own bounty (20,000 lamports flat; `SCRIPT_BOUNTY * min(n, 8) / 8`
  on a step with an n-entry answer), taken out of the piles in the order dip, lp, holders, owed.
- One action per step besides the builder payout: a dip buy if the rule fires; else a liquidity
  add if due; else holder payouts if holdings and the terms are passed. The builder wallet is
  paid, within the terms, on any step without a holder answer.

## Additive items (owner approved "additive gamification", 2026-10-21)

- `NEVER_SOLD_BONUS_BPS = 1_000`: a holding that never sold into the pool since it was funded
  weighs 10% more. One constant; 0 turns it off. Not in DIP-VAULT.md v5.
- `BURN_DIP_BUYS` constant (true): the owner asked "maybe no burn?" on 2026-10-21. With false,
  bought coins stay in the companion's holding and are never sold by this script.

## Open decisions for the owner

1. The builder scale steps (proposed above).
2. Burn or keep dip-bought coins (`BURN_DIP_BUYS`); the listing words say "burn".
3. Keep the never-sold bonus at 10%, change it, or drop it.
4. Payout terms in `keeper.json`: stepBps 2000, holdingBps 10000, windowBps 10000, windowSecs
   86400. These bound the companion's checks only; the script's own piles are the real limit.
5. Whether to install the Solana toolchain here (about 5.4 GB) to build SBF and run T9.
6. Quote tokens: upstream `bordrless-programs` shipped "coins launched against SPL Token or
   Token-2022 mints" on 2026-10-20 (commit d31557a, after the pin). This build assumes native SOL
   quotes (`launch.is_native()` required). A stock- or basket-quoted launch would need a new pass.

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
- All of the above is host-tested logic. No LiteSVM run, no trading test, no mainnet.

## How to reproduce

    cd harness && cargo test            # 161 passed
    python3 -I tools/make_variants.py   # writes dist/dipvault/{hook,script}
    python3 -I tools/scan_rules.py dist # clean

Submission files: `dist/dipvault/hook/src/*.rs` + `hook.json`; `dist/dipvault/script/src/*.rs` +
`keeper.json`; words in `variants/dipvault/notes.txt` (596 chars). The owner submits.
