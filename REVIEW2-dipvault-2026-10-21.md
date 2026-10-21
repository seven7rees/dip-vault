# Dip Vault: scoped independent review (2026-10-21, reviewed at 091dc22)

Reviewer: a separate session with no shared context. Verdict as given: FAIL at 091dc22 (blocker 1).
Status after fixes in ab98ee0: findings 1 to 8 and 13 fixed; 9, 10 open and flagged; 11, 12, 15, 16 noted
in DIP-VAULT-BUILD.md; 14 fixed in the report.

1. BLOCKER. The same coins could be paid many times in one week: emptying a holding clears its
   record and a fresh wallet starts at round 0, so coins moved every 6 hours were paid again.
   Fix (ab98ee0): only a holding whose clock started before the round began is measured or paid
   (`holders::held_since_round_start`). Residual: coins moved into a large old holding not yet
   measured this week ride its clock; needs a per-round balance snapshot the hook does not keep.
2. MUST FIX. A step sender leaving a holder's wallet out, or passing it read-only, turned the
   payout into a touch and cost the holder its week. Fix: the holding is skipped, not touched.
3. MUST FIX. dist/ was stale. Fix: regenerated and rescanned; `diff -rq` dist against src is empty.
4. SHOULD FIX. The liquidity buy could exceed the pool cap when the cap was under the least buy.
   Fix: no buy when the cap is under the least buy; the buy is clamped to the cap last.
5. SHOULD FIX. A `DepositTooSmall` on the add reverted every later liquidity step. Fix: the LP the
   DEX would mint is computed first (`liquidity::lp_for`); an add that would mint none is skipped.
6. SHOULD FIX. The "holders waiting" check matched the creator's holding and the pool's vault.
   Fix: it uses the payout loop's filter.
7. SHOULD FIX. A repeated or read-only holding failed the whole step. Fix: skipped.
8. SHOULD FIX. Dust holdings could fill the 8 slots. Fix: keeper.json minBalance 1,000,000,000
   base units (1,000 coins at 6 decimals).
9. SHOULD FIX (open). The registry names two fixed keys (the pool's LP mint, the launch's LP
   holding); the hook brief's rule text allows fixed keys only for the system program, a sysvar
   or the token program. LiquidityTek was approved with the same two. Confirm before submitting.
10. NOTE (open). `record_payout` writes state outside prepare and the callbacks; the AI reading may
    mark it (pattern 4 of the pair brief requires it).
11. NOTE. A top-up of a large old holding barely moves its clock, so it can sell within 10 minutes
    without the brake: the spec's own averaging rule.
12. NOTE. A holder's own liquidity add sets its sold flag.
13. NOTE. Comments: holders.rs fixed; LP pace now 24 adds a pile (about 4 hours at one add per 10
    minutes).
14. NOTE. Report contradictions fixed.
15. NOTE. T9 builds the step's accounts by hand; the keeper's own set is unverified until a launch.
16. NOTE. Owner decisions: the bounty comes out of the dip pile first; in a long decline the dip
    rule wins every step.

Verified correct by the reviewer: `pairing()` equals the DEX's `lp_for_deposit` pull in both
branches, so no wrapped SOL is left behind; no overspend, no dev bag taken; the payout limits
match the companion's; the bounty and pot accounting match; the keeper filter matches the round
field; the relay holds against the DEX sell ordering; `quote_cut` fails no valid call.
