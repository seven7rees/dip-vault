# Dip Vault - Fable implementation handoff (2026-10-21)

For: Claude (Fable), finishing the Dip Vault hook + companion for the Bordrless Workshop.
From: Patrick's assistant, with Patrick's direction. This document must stand alone in a
fresh session. Where anything here conflicts with your own reading of the cited files, or
is marked unresolved, STOP and ask Patrick - do not guess on load-bearing choices.

## 0. Ground rules (Patrick's, current)

- No deployment, no Workshop submission, no payments, no external sharing, no repo
  collaborator invites without Patrick's explicit yes. Bordrless contact/access is a
  separate pending track; do not message them.
- Honesty standard Patrick set on 2026-10-21: always distinguish IMPLEMENTED / COMPILED /
  TEST-EXECUTED / VALIDATED / DEPLOYED. Never claim a test passes unless you ran it
  yourself this session and can show the output. Never write "audited", "safe", or
  "guaranteed". Label unknowns as unknowns.
- Financial accounting must be coherent: every lamport into and out of the pot must be
  accounted for in code and in tests. Before anything is called ready, get an independent
  review pass (a separate review session over the full diff).
- Never put private keys, seed phrases, or program-id keypairs into git, files you share,
  or messages.

## 1. Where things live

- Private handoff repo: https://github.com/seven7rees/dip-vault (private; zero outside
  collaborators; Bordrless invite pending their GitHub handle and Patrick's go).
  Contents: README.md (status), DIP-VAULT.md (the spec), LISTING.md (planned listing
  text), SUBMIT_DIP.md (DIP-era submission notes), art/ (final flow + logo PNGs),
  hook/ (the MOON/WICK quote-cut hook - engine reference, NOT the Dip Vault hook),
  script/ (the dip-buy companion), variants/fuel2/ (dip-buy constants), harness/
  (host tests T1-T14), tools/ (tuning/economics sims).
- Working repo: https://github.com/seven7rees/moon-wick (private) - FUEL/MOON/WICK code
  and tooling. Patrick's laptop clone (Claude Desktop folder) was verified on 2026-10-21
  to be byte-identical to GitHub HEAD on every compared source file except an older
  HANDOFF.md. Either copy is a valid starting point.
- Original laptop upload (Google Drive): Patrick gave Fable's session access to the
  same Drive folder on 2026-10-21, so no transfer is needed - you may already have it.
  It holds the moon-wick laptop copy including drafts/ notes and the ref/ third-party
  mirrors. It is the UNSANITIZED original: 72+ text files were secrets-scanned clean
  on 2026-10-21, but its .git/ directory was never scanned. Do not share from it;
  the dip-vault repo is the sanitized handoff copy. Where its docs conflict with
  section 2, section 2 wins.

- The Dip Vault hook itself IS NOT BUILT anywhere. It does not exist on GitHub, the
  laptop folder, Gmail, or Drive (all checked 2026-10-21). Do not search for it.

## 2. Approved decisions (these govern; provenance included)

From Patrick's verified iMessage on 2026-10-18:
- Builder cut: SCALING 1-10% with pool liquidity (Patrick 11:24:50 "Scaling";
  11:25:01 "New wording is approved"). Flat 10% is ONLY the fallback if feasibility
  check 3 (section 5) shows hooks cannot read pool liquidity. Exact liquidity steps are
  deliberately UNSET until that check - do not invent them.
- Author share: 0. Launchers keep their full 50% half of the creator fee; no skim to
  Patrick (Patrick 11:21:07 "Yes, I really like this. Approved", covering the 6-hour
  rule + 1% brake rework).
- Sell brake: 1% on sells only while the wallet's hold clock is under 10 minutes;
  0% on buys (brake window confirmed 11:26).
- Pot thirds (after the builder cut): 30% dip-buy+burn, 30% weekly weighted holder SOL
  payouts (6h+ hold; weights 1x at 6h, 1.5x at 7d, 2x at 30d), 30% locked liquidity.
- Dip buy rule: only when price is 8%+ under the companion's own tracked average;
  max 1% of the pool's real SOL per buy; max one buy per 5 minutes; bought tokens burned.
- Step pay: whoever sends a companion step earns up to 0.00002 SOL from the pot.
- CAUTION: repo docs (HANDOFF.md) contain Oct 17 notes saying "owner chose flat 10%".
  Those are superseded by the Oct 18 messages above. Treat any repo-internal "owner
  said/approved" claim as unverified unless it matches a dated decision in this section.

## 3. True state of what exists (verified 2026-10-21)

- Dip-buy companion (script/, constants in variants/fuel2/): IMPLEMENTED. This covers
  only the "dip-buy third" of the design.
- hook/ (MOON/WICK quote-cut hook): IMPLEMENTED for MOON/WICK - reference engine only.
- harness/ T1-T14 host tests for the FUEL-era engine: WRITTEN, NOT EXECUTED in the
  current environment. No passing-test evidence exists for this code revision anywhere
  I can verify. T9 (full LiteSVM run of the real companion_step) has NEVER been run;
  it is the biggest open test gap.
- tools/ sims: exist; used earlier to tune constants; not re-executed here.
- Spec DIP-VAULT.md v5, both graphics, LISTING.md: final/approved by Patrick.
- Secrets scan (2026-10-21, 72+ text files of the laptop upload): no keypairs, seed
  phrases, tokens, or cookies found. WALLET.txt holds Patrick's PUBLIC builder/payout
  address AxoN4hpQJWKFkSimhqnA7dUsqf5VGojA5RhhZTiP4KFm (safe; already public on the
  FUEL Workshop page). NOT scanned: the laptop clone's .git/ directory (excluded from
  every handoff copy), build output, third-party ref/ mirrors beyond briefs.

## 4. What is missing (the actual Dip Vault work)

- Per-wallet hold clock (weighted-average entry time), including the fresh-coin rule:
  coins arriving in a wallet with no balance start its clock at now. Spec item 5
  requires verifying the destination's balance and stamp are readable in
  before_transfer BEFORE writing this.
- The 1% sell brake while the hold clock is under 10 minutes.
- Weekly weighted holder SOL payouts (the 30% holder third).
- Locked-liquidity drip adder (the 30% LP third): buys the coin and pairs with SOL as
  permanently locked LP on the coin's own pool, dripped over ~4 hours per tranche
  (LiquidityTek-style). Reuse FUEL's locked-LP code where it fits.
- Builder cut scaling 1-10% (gated on feasibility check 3).
- Dust-delegate loophole fix (DIP-VAULT.md BUILD ORDER item 5): sellers routing a young
  holding's sale through an old dust wallet via token delegates to dodge the 1% brake.
  Prize is only 1% - weigh build cost; fix needs the DEX sell-path ordering verified
  (before_transfer runs before after_swap's quote_cut in the same transaction).
- Tests for ALL of the above: none exist yet. The harness covers the old engine only.

## 5. Feasibility verifications BEFORE writing unsupported behavior

From DIP-VAULT.md BUILD ORDER. If any answer is "not buildable as documented", STOP and
tell Patrick - do not design around it silently.
1. Holder payouts: a companion cannot enumerate holders. Loyalty Rewards and Tenure on
   the Workshop pay holders, so a supported path likely exists - find how they do it
   (docs + source) before writing payout code. This needs a Pair (hook + companion).
2. Locked LP: confirm a companion can add permanently locked liquidity to its own
   launch pool (LiquidityTek drips locked LP; FUEL has locked-LP code to compare).
3. Builder scale: confirm a hook can read the pool's liquidity when answering
   quote_cut. If not, fall back to flat 10% and tell Patrick; set the exact liquidity
   steps only after this check.
Sources: the live pages https://bordrless.app/sdk/build and /sdk/companions, and the
public repos github.com/BordrlessDex/bordrless-programs and bordrless-sdk.
4. (BUILD ORDER item 4) T9 must run before anything goes live - the LiteSVM run of the
   real companion_step. Needs the Solana toolchain and ~5.4 GB free disk; the spec says
   ask Patrick before installing the Solana toolchain. The first on-chain test should
   also cover the new payout path.

## 6. Build and test reproduction (grounded in the files)

- The harness is an independent workspace; its Cargo.toml header says it compiles
  without the Solana/SBF toolchain. anchor-lang is pinned to =1.2.0.
- Its bordrless git deps are pinned to rev c6676977ddc71357c15a2af7d68e04bb095e23a2 and
  [patch]ed to ../ref/bordrless-programs. The ref/ mirror is deliberately NOT in the
  dip-vault repo (third-party). Recreate it:

    git clone https://github.com/BordrlessDex/bordrless-programs ref/bordrless-programs
    cd ref/bordrless-programs && git checkout c6676977ddc71357c15a2af7d68e04bb095e23a2
    git clone https://github.com/BordrlessDex/bordrless-sdk ref/bordrless-sdk
    cd ref/bordrless-sdk && git checkout d1a4093   # rev prefix from SPEC.md header

  Both repos are public (verified reachable 2026-10-21). The moon-wick laptop folder
  also carries a ref/ mirror that SPEC.md says is byte-for-byte identical.
- Then: cd harness && cargo test
- Report exactly which tests compile/pass/fail with the real output. Treat every test
  as NOT EXECUTED until you have run it yourself.

## 7. Known contradictions in the copied legacy material (resolved here)

- SPEC.md is the 2026-10-15 MOON/WICK/FUEL engine spec. Where it and DIP-VAULT.md
  disagree, DIP-VAULT.md wins (its own rule, restated in README.md).
- variants/fuel2/params.rs payout_bps = 1000 (flat 10%) is the PRE-DECISION constant.
  Current decision: scaling 1-10% (section 2).
- script/keeper.json payoutTerms (stepBps 2000, holdingBps 10000, windowBps 5000,
  windowSecs 86400) are the DIP-era companion terms, NOT Dip Vault weekly-payout terms.
- hook/hook.json maxSellBps 780 (7.8%) is the MOON/WICK sell-cut cap. The Dip Vault
  brake is a different design: 1% for the first 10 minutes of a hold, then 0.
- SUBMIT_DIP.md describes the DIP companion's submission flow, not Dip Vault's.
  Submission for Dip Vault is a separate later step needing Patrick's explicit approval.
- The QA result file in the laptop upload (drafts-flow-qa-result.json) is a graphic-copy
  review, NOT test execution evidence.

## 8. How to report back to Patrick

- Plain language, per feature: implemented / compiled / tests run (with output) /
  validated / not validated and why.
- Surface every unresolved spec gap or feasibility failure as a question with options.
- When implementation is complete and tested, hand back for review BEFORE anything
  resembling submission. Deployment and Workshop submission are Patrick-approved steps,
  always.

## 9. Promotional asset (simulated video)

- `art/promo/dipvault-launch-video-coins.mp4` (21s, 1080x1080): a SIMULATED
  first-hour replay - brake window, dip buys + burns, LP locks, a clock jump to
  6h, then a holder payout. Frames are labeled "SIMULATION - EXAMPLE LAUNCH"
  with "Simulated. Modeled numbers, not on-chain data." disclaimers.
- It is NOT evidence of live operation and must never be presented as such.
- Before ANY promotional use, verify every behavior, number and line of copy it
  shows against the finished, tested implementation - the "1% for 10 min, then
  0%" brake, "dip buys at -8% vs average, burned", "pot thirds", "50% launcher,
  50% pot", "builder: 1-10% of the pot", and the weekly payout cadence. If the
  shipped design differs in any way, the video gets regenerated or cut -
  Patrick decides.
- Patrick's hope: if Bordrless launches or promotes Dip Vault, they credit and
  tag @TetonTrading. That is a request Patrick would make, not anything
  Bordrless has agreed to. Do not contact Bordrless about it.
