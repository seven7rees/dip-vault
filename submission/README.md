# Dip Vault: a Bordrless Workshop pair, ready to submit

Builder: Patrick (@TetonTrading). Payout address (public): AxoN4hpQJWKFkSimhqnA7dUsqf5VGojA5RhhZTiP4KFm.
Item name: Dip Vault. Kind: pair (Both). Builder's share at submission: nothing (author share 0):
the builder's only earning is the pot cut written into the script (BUILDER_WALLET in
script/src/params.rs), so whoever's wallet submits changes nothing about who is paid.

## The files, exactly as the submit page takes them

- hook/src/lib.rs, hook/src/clock.rs, and hook/hook.json beside them (SOL cut caps: buys 1 bps,
  always answered 0; sells 80 bps). Two files, 28,127 bytes. One declare_id!, the standard prepare.
- script/src/*.rs (8 files, 89,917 bytes) and script/keeper.json beside them.
- notes.txt: the builder's words, 595 characters.

Both halves build on the Workshop templates with no added crate. The declare_id! values are local
build addresses; the builder writes the real ones.

## What it does, in one breath

A hold clock in every holding (weighted-average entry time, written by before_transfer). The hook
answers 80 bps on a sell only while the seller's clock is under 10 minutes, 0 after, 0 on buys; a
sell's real source clock is relayed through the hook state so a delegate sale cannot dodge it. The
companion script keeps the pot in four piles that always equal the pot: the builder's fifth of
new pot money; dip buys (8% under its own reference, 1% of the pool, 5-minute cooldown; coins kept);
weekly holder payouts through the companion's payout answer (the hook's record_payout records
them; weights 1x/1.5x/2x by hold time, +10% never sold; a holding counts if held since before the
week began; paid the week after it is measured, by the measured basis, so shares never exceed the
week's budget); locked liquidity after graduation (the coins held paired with SOL, LP minted into
the launch's own LP holding, as LiquidityTek does).

## What has been checked (and what has not)

- 165 host tests pass (layouts pinned against the programs' own crates at c6676977, every rule
  and bound, ABI pins of wrap_sol and add_liquidity byte for byte).
- The whole story runs in LiteSVM, twice: against the programs built from the pinned source and
  against the live mainnet program bytes dumped 2026-10-21: prepare, a quote-cut config, a native
  script launch with terms, buys, the brake (79 to 80 bps of the SOL out), a free late sell, a real
  delegate sale paying the brake, the builder paid, a dip buy kept, graduation, a locked-liquidity
  add at the pool's exact ratio with no wrapped SOL left, a week measured, the next week paid in
  full to every holder, the piles equal to the pot after every step.
- Not run: the Workshop's own rule scan, build, trading test and reading. Not audited.

## Three things to confirm on your side

1. The hook's registry lists two fixed keys beside its state: the pool's LP mint and the launch's
   LP holding (LiquidityTek's shape), so the companion's liquidity adds mint LP into the launch's
   holding with no extra account from the keeper. The hook brief's rule text names only the system
   program, a sysvar or the token program as fixed keys.
2. record_payout (pair brief, pattern 4): an instruction beside prepare and the callbacks that
   writes a holding's record through write_hook_data, signed only by the companion's payout
   authority, exactly as loss_cover_hook does.
3. Holder reach: with the swap accounts, the payout accounts and the protocol lookup table, one
   holding fits a step (1,219 of 1,232 bytes measured), so keeper.json asks for one a step
   (perStep.withSwapAccounts 1). If the keeper gives a trading-and-paying pair a lookup table of
   its own, more fit and the number can go up.

## Owner decisions already made

Builder's share a fifth of new pot money (LiquidityTek's rate); never-sold bonus 10%; no burn
anywhere; launcher share 5,000 bps; payout terms open (the script's own piles bound every payout).
