# Dip Vault: package review 3 (fresh eyes, 2026-10-21)

Pinned to moon-wick `f2d25ca` and dip-vault `991492c`. The repos changed three times during this
review (091dc22, ab98ee0, f2d25ca, plus uncommitted edits at 10:46 to 10:50). Every finding below
was re-checked at f2d25ca. Rust logic line by line is out of scope (other reviewer). Probes were
run on copies under /tmp (git archive of HEAD, script .so rebuilt from HEAD); no repo file was
edited, except that dist/ was regenerated as asked.

## Findings

1. BLOCKER. litesvm/tests/t9_dipvault.rs:456-460 (code: script_variants/dipvault/src/lib.rs:555-603).
   Defect: in the week-two paying step, 6 of the 8 holders are paid 0 and marked done for the week.
   Probe at HEAD: pays [178,693,642, 4,704,564, 0, 0, 0, 0, 0, 0]; the step budget (20% of the pot,
   183,398,206) is used by the first two; the other six get 0-lamport entries, their hook record
   round becomes 2978, so the keeper filter (round != epoch) never brings them again that week.
   c had 32% of last week's weight and got 0. `total > 0` and `rec.paid == gained` (0 == 0) pass.
   Why: the report (DIP-VAULT-BUILD.md:35 "the next week a step pays them") and commit ab98ee0
   ("T9 passes with eight holders paid") claim the opposite of what the run shows, and the listing
   promises weekly SOL to holders.
   Fix: assert every holder with a due of at least MIN_PAYOUT_LAMPORTS gets pay > 0 or is left
   unmarked; logic reviewer to stop emitting a 0 entry when step_left cut the pay to 0.

2. MUST-FIX. t9_dipvault.rs:446 ("nothing twice in a round") and :485-486 ("moved coins earn
   nothing", "z is not even marked"). Defect: both steps were liquidity adds (probe: one
   LiquidityAdded event in each). A liquidity step spends, and the payout loop runs only when
   `done.spent == 0` (lib.rs:554), so these assertions pass whatever the round dedupe and the
   round-start rule do. Why: the fix for review 2's blocker has no real on-chain evidence.
   Fix: assert no LiquidityAdded in those steps (or make lp not due), and put a payable holder in
   the same step as z so a payout answer is actually produced.

3. MUST-FIX. script_variants/dipvault/keeper.json `holdings.perStep.withSwapAccounts` 8.
   Defect: the T9 step with 8 holders has 80 accounts and is 2,155 bytes on the wire (v0, no
   lookup table; probe via `env.v0_size` and `tx.size`), against the 1,232-byte packet limit and
   above a 64-account lock limit. LiteSVM enforces neither. Whale Bait's brief value is 3.
   Why: "8 with payouts and swapAccounts" is not shown to fit one transaction; the keeper would
   bring fewer, which cuts weekly reach.
   Fix: size the step with the protocol's lookup table (`send_v0`/`v0_size`) and set perStep to
   what fits.

4. MUST-FIX. litesvm/Cargo.toml, DIP-VAULT-BUILD.md:21-24. Defect: T9 runs Bordrless programs
   built from ref @ c667697 (2026-10-16), not the live programs. ref/briefs/pair-brief.txt:104-106
   requires "the live programs copied from mainnet (solana program dump)"; upstream shipped
   d31557a on 10-20. Fix: dump the mainnet programs and rerun T9, or record that their hashes
   equal the local builds.

5. MUST-FIX. keeper.json root (no `roundSecs`). Defect: /tmp/keeper-latest.ts:254-255 "roundSecs:
   the round the keeper's cursor rotates by; null: none (the cursor never starts over)". Whale
   Bait pairs its epoch filter with roundSecs (pair-brief.txt:70). Why: weekly payouts need the
   cursor to start over each week. Fix: add `"roundSecs": 604800`.

6. SHOULD-FIX. keeper.json hookData filter vs lib.rs:575-577 and holders.rs:7-8. Defect: the
   keeper brings any holding whose round != epoch; the script also skips a holding whose clock
   started after the week began, without marking it, so that holding is brought again every step
   all week and takes a slot (the z case). holders.rs:7-8 says such coins "are marked"; the code
   does not mark them. Why: wasted slots; a comment that contradicts the code is what the AI
   reading flags. Fix: touch them with 0, or correct the comment and accept the slot cost.

7. SHOULD-FIX. keeper.json view "Liquidity locked (SOL)" (offset 201, lp_spent). Defect: lp_spent
   is all lamports the liquidity step spent, including SOL used to buy coins on the shortfall path
   and its fees. Fix: label it "SOL into liquidity".

8. SHOULD-FIX. t9_dipvault.rs:372. Defect: `s2.dip_pile < s1.dip_pile + (pot delta)` holds with no
   spend at all (probe: 132.5M < 15.2M + 375.9M; the dip pile gets about a third of new money).
   1/48 of the pile and the 1%-of-pool cap are not checked. Fix: assert the exact pile change
   and spent <= 1% of pool real SOL.

9. SHOULD-FIX. t9_dipvault.rs:295, :327. Defect: the brake is checked only as "accrued grew", not
   80 bps; "about 1% trader-facing" (the launchpad's quarter) is checked nowhere. Probe: cut
   824,527 lamports on a seller net of 99,453,689 (about 82 bps of net plus cut), consistent with
   80 bps of gross. Fix: assert the cut equals 80 bps of the SOL out within rounding.

10. SHOULD-FIX. t9_dipvault.rs:298. Defect: b sells in the same second it bought, so
    `rb.stamp == now` also passes if a sell reset the clock. Fix: warp between buy and sell.

11. SHOULD-FIX. t9_dipvault.rs:355-359. Defect: "eight steps that raise the reference" is not
    asserted (ref_price never compared). Fix: assert ref_price rises across the loop.

12. SHOULD-FIX. t9_dipvault.rs:412-413; DIP-VAULT-BUILD.md:48. Defect: the LiquidityAdded event is
    discarded and the pool ratio is not asserted (probe: 2.25e-4 for both, so correct this run);
    lp_spent equals quote_amount and no swap ran, so the shortfall-buy path was never exercised,
    yet the table labels the step "(buy, wrap, add)". dev_tokens is 0, so "never the dev bag" is
    not exercised; nothing tries to withdraw the locked LP. Fix: assert the ratio, add a run with
    a shortfall and one with a dev bag, fix the label.

13. SHOULD-FIX. t9_dipvault.rs:345-347. Defect: builder share and thirds checked only as > 0, not
    100 bps (pool under 5 SOL) and 3,333/3,333/3,334. Fix: assert the exact split of the first step.

14. SHOULD-FIX. DIP-VAULT-BUILD.md:36 "After every step the script's four piles equal ...".
    Defect: `piles_equal_pot` is not called after the 8 reference steps, the same-week step, the
    first try of `payout_step`, or the z step. Fix: call it inside `step()`.

15. SHOULD-FIX. DIP-VAULT-BUILD.md:43-50. Defect: CU is not deterministic; across 6 runs I saw dip
    289,499 to 320,999, liquidity 304,409 to 337,405, measuring up to 457,081, paying up to 456,283,
    against single figures in the table. moon-wick target-sbf/dipvault_script.so (10:50:47)
    predates commit ab98ee0 (10:51:33), and dipvault_hook.so (09:56:32) predates the clock.rs
    change (10:33); no hash ties a binary to a commit. Fix: report a range and the max; rebuild
    at the commit and record sha256.

16. MUST-FIX. harness/tests/t16_dipvault_piles.rs:120, :124; DIP-VAULT-BUILD.md:114. Defect: the
    5,000-step simulation re-implements the step and its two invariants are true by construction
    (reconcile forces total == pot_left; in_total == out_total + pot is arithmetic on the test's
    own variables). The report says it "checks every lamport in equals every lamport out". Fix:
    reword the report; check pile totals before reconcile, or drive the real handler.

17. SHOULD-FIX. harness/tests/t19_dipvault_layout_rules.rs:243. Defect: any "not audited" in a file
    turns off the "audited" check; " safe " misses "safe." and "Safe,"; DIP-VAULT-FORMAT-NOTE.txt
    and the report are not scanned. Fix: strip "not audited" first, match on word boundaries,
    scan every text file shipped.

18. SHOULD-FIX. harness/tests/t17_dipvault_holders.rs (a_week_measured_then_a_week_paid). Defect:
    it asserts paid > 4M of 5M; nothing covers the step-budget cut that marks later holders for 0
    (finding 1). Fix: a host test of the step loop with step_left exhausted.

19. NOTE. t19 "the_hook_calls_no_program" scans text and cannot see the CPI inside
    `bordrless_game::write_own_hook_data`; the report already says so. The T15 delegate test
    builds the relay values by hand; the T9 delegate sale (t9:306-330) now covers it on chain,
    and it means something: the launchpad passes the signer's holding to quote_cut
    (bordrless_launch hooks.rs:206), so without the relay the cut would be 0.

20. MUST-FIX. variants/dipvault/notes.txt, DIP-VAULT-FORMAT-NOTE.txt:8. Defect: "No early-sell
    tax" sits beside "a 1% brake only in its first 10 minutes", which is an early-sell tax.
    Fix: "No long sell tax: a 1% brake only in the first 10 minutes".

21. MUST-FIX. notes.txt "Buys pay 0%", "then nothing"; FORMAT-NOTE:40-41. Defect: buys and sells
    still pay the launch's creator fee and LP fee (T9 config: 200 bps creator fee); only the
    hook's cut is 0. Fix: "no extra cut on buys", "no brake after 10 minutes".

22. MUST-FIX. notes.txt "hold 6 hours and get paid", "weekly SOL to holders"; FORMAT-NOTE:8, :27.
    Defect: since ab98ee0 a holding counts only if its clock started before the week began
    (holders::held_since_round_start; weeks are floor(now/604800), so they start Thursday
    00:00 UTC), and a top-up during the week moves the clock and skips that week. Holdings under
    1,000 coins are never brought (minBalance 1,000,000,000 at 6 decimals), payouts under
    0.0001 SOL are not sent, and per finding 1 holders after the step budget is used get 0.
    Fix: state the week-start rule and the 1,000-coin minimum; drop "get paid" as a promise.

23. MUST-FIX. FORMAT-NOTE:27. Defect: "Week one measures everyone"; "each week pays by the weights
    measured the week before"; "No claims needed: the companion pays the wallet directly". The pay
    is this week's weight over last week's total, first come first served (holders.rs:10-12), at
    most 8 a step, and conditional (findings 1, 3, 22). Fix: "holders the keeper reaches are paid
    by weight, from a weekly budget, while it lasts".

24. MUST-FIX. FORMAT-NOTE:29; script_variants/dipvault/src/params.rs:129, :132. Defect: "about 1/48
    of this pile (a four-hour drip)" while LP_PACE_STEPS is now 24. Each add takes 1/24 of what is
    left, so about 36% of a pile remains after 24 adds (4 hours); "every other step" holds only
    when no dip buy fires. Fix: "1/24 of the pile every 10 minutes after graduation".

25. MUST-FIX. FORMAT-NOTE:50. Defect: "161 host tests" (now 163) and "T9 is in progress" (it
    passes); LiteSVM is a local simulation, not "on-chain". Fix: update.

26. SHOULD-FIX. "for ever", "nobody can spend" (notes.txt; FORMAT-NOTE:11, :29;
    script_variants/dipvault/src/lib.rs:16; DIP-VAULT-BUILD.md:78). Defect: the lock rests on
    Bordrless's launch program, which Bordrless can upgrade with its own key (pair brief), and on
    its docs (hooks-v2.md:272), not on this pair's code; no test tries a withdrawal. Fix: "locked
    in the launch's own LP holding, which the launchpad does not spend".

27. SHOULD-FIX. FORMAT-NOTE:8 "Nothing is ever burned: every coin the pot buys becomes locked
    liquidity". Defect: holders can still burn (before_burn kept); coins bought before graduation
    and unpaired coins stay held; a coin that never graduates never adds liquidity. Fix: "The pot
    never burns. After graduation its coins are paired into locked liquidity".

28. SHOULD-FIX. FORMAT-NOTE:17 "The dust-delegate trick is closed". Defect: the relay works only
    when the hook recognises the pool (30 bps LP fee assumption, DIP-VAULT-BUILD.md known risks).
    Fix: "addressed".

29. NOTE. FORMAT-NOTE:37 contains "Audited. Safe. Guaranteed." as a do-not-use list, so the
    file breaks the literal forbidden-word rule.

30. NOTE. notes.txt is 600 bytes with its trailing newline (make_variants reports 599 chars);
    DIP-VAULT-BUILD.md:207 says 596. No headroom against the 600 limit.

31. NOTE. script_variants/dipvault/src/lib.rs:24 "Coins never leave the companion's holding except
    into the pool". The dev bag sits in the same holding and vests out.

32. MUST-FIX. DIP-VAULT-BUILD.md stale lines: :13 "T15-T19 (40 tests)", "161 passed" (now 42 and
    163); :202 "161 passed"; :34 "three holders" (T9 uses 8); :48 "(buy, wrap, add)" (no buy
    ran); :185 "a swap, a wrap, an add and a burn ... Not measured here" (no burn, and it was
    measured); :114 (finding 16); :207 "596 chars"; open decisions numbered 1, 3, 4, 6. The
    section "T9: what the LiteSVM run proves" lists claims that findings 1, 2 and 8 to 14
    show T9 does not establish. Status labels are otherwise honest: nothing is claimed
    VALIDATED or DEPLOYED.

33. MUST-FIX. /Users/patrickdavis/Desktop/claude/dip-vault/README.md:3-5, :13, :15-18, :32.
    Defect: says "NOT built with the SBF toolchain, NOT run in LiteSVM", "bought tokens burned",
    "T9 ... has never been run", "harness ... not yet executed", "161". Fix: rewrite to the
    current state.

34. MUST-FIX. dip-vault repo. Defect: WALLET.txt is missing, so harness/tests/t18_dipvault_abi.rs:211
    `include_str!("../../WALLET.txt")` does not compile; tools/fetch_refs.sh is missing although
    .gitignore:1 says to recreate ref/ with it. Neither the harness nor T9 can be reproduced from
    the handoff repo. Fix: add both (WALLET.txt holds a public address only).

## What I verified

- T9 at f2d25ca passes (script .so rebuilt from HEAD in /tmp): 1 passed. The T9 claims were
  probed one by one with instrumented copies (findings 1 to 15).
- Host tests: `cd harness && cargo test` gives 163 passed, 0 failed; T15 to T19 have 42 tests.
- `python3 -I tools/make_variants.py` then `python3 -I tools/scan_rules.py dist`: clean. dist/dipvault
  matches the sources byte for byte. Hook: 2 .rs files, 27,595 bytes. Script: 8 .rs files (at the
  limit), 86,968 bytes. Both under 120 kB. One `declare_id!` in each lib.rs, lower-case src names,
  hook.json `{maxBuyBps 1, maxSellBps 80}` present and equal to MAX_BUY_BPS/MAX_SELL_BPS and
  BRAKE_BPS.
- keeper.json against /tmp/keeper-latest.ts: every key is allowed; version 1; minBalance a digit
  string; hookData offset 1 width 4 (at most 60) "ne" against epoch 604800; payTo "owner" (needed
  with payouts); perStep 8 within 1 to 16 and present for swapAccounts true; the builder wallet is
  valid base58 and not a protocol key; stepSecs 300, stateLen 320 (at most 10,240) and payoutTerms
  are within SCRIPT_BOUNDS and PAYOUT_TERMS_BOUNDS; launcherShareBps 5000 (at most 9,500); 6 views
  (at most 6), labels 24 characters or fewer, width 8, valid `as`; 1,565 bytes (at most 4,096).
  I did not execute the TS validator. View offsets 57/65/73/89/193/201 match the ScriptState
  comment and the Borsh layout (T19 pins them). The hook round field (clock.rs offset 1, u32 LE)
  equals floor(now/604800) as written by record_payout (probe: rounds 2977 and 2978).
- Numbers in notes.txt and the format note checked against params.rs/clock.rs: 80 bps, 600 s,
  8%, 1% of the pool, 300 s cooldown, 6 h/7 d/30 d tiers 1x/1.5x/2x, +10%, 3,333/3,333/3,334, the
  builder scale 1/2.5/5/7.5/10% at 0/5/20/50/100 SOL, 20,000-lamport bounty, 50/50 fee. These
  match; the 1/48 figure does not (finding 24).
- Feasibility citations exist at the cited paths: docs/companions.md, payout.rs `pay_holdings`
  :595, loss_cover_hook/script, swap liquidity.rs:111 CurveLocked, hooks-v2.md §3.1 (:247, :322),
  companion.ts:563 `unwrapAccounts`, script.rs:214 `custody_intact`, ref/site/lt_now.json,
  bordrless-hook lib.rs:232 quote_reserve and :193/:201 destination fields, and the 600,000 CU line
  in ref/site/sdk_companions.html:116. Live launch counts (Loyalty Rewards, Lossback,
  LiquidityTek) were not verifiable offline.
- Handoff sync: dip-vault 991492c matches moon-wick f2d25ca for hook_variants, script_variants,
  variants/dipvault, harness/tests t15 to t19, litesvm/tests, DIP-VAULT-BUILD.md, the format note,
  make_variants.py and scan_rules.py (only sim_dip_tuning.py differs).
- Secrets: no tracked or historical keypair files in either repo, no 64-byte arrays and no
  base58 secret strings; target-sbf/*-keypair.json are ignored and untracked.

BLAST RADIUS: reversible-wide
VERDICT: FAIL
ISSUES: findings 1 to 34 above; blocker 1, must-fix 2, 3, 4, 5, 16, 20 to 25, 32 to 34
RISK: holders are promised weekly SOL, but in the only end-to-end run 6 of 8 eligible holders got 0 and were locked out for the week, and the test passes anyway.
