# Package review (REVIEW3-package-2026-10-21.md): status of the 34 findings after the fixes

Fixed in code (script/hook): 1 (the blocker: holders beyond a step's budget are skipped, never
marked 0; and the overpay behind it: payouts use the measured basis, the hook records the balance
measured); 5 (`roundSecs` 604800); 6 (late clocks are touched, comment corrected); 7 (view label
"SOL into liquidity"); 3 (`perStep` 1, measured: one holder fits 1,219 of 1,232 bytes with the
live protocol table; the test asserts keeper.json matches what fits).
Fixed in tests: 2 (the round checks run on payout-turn steps with a payable holder beside z);
8 (exact dip spend, at most 1% of the pool); 9 (the brake 79 to 80 bps of the swap's SOL out);
10 (warp between b's buy and sell); 11 (the reference rise asserted); 12 (the pool ratio and
`lp_minted` asserted, no stranded wrapped SOL; the shortfall buy is NOT exercised, stated); 13
(the exact split of the first step); 14 (`piles_equal_pot` inside `step`); 16 (T16 checks the
piles before reconcile); 17 (whole-word scan of every shipped text, "not audited" removed first);
18 (a host test of the step loop out of budget, and of the measured-basis bound).
Fixed in words and docs: 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34.
Run against the live programs: 4 (T9 passes against the mainnet dumps too; hashes differ from
the local builds, both runs recorded).
Noted, not changed: 15 (CU reported as a range; binaries hashed at the commit); 19.
