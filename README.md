# Dip Vault - design spec + partial implementation (private review copy)

Status: design finalized (DIP-VAULT.md, v5, 2026-10-18); implementation PARTIAL.
Not submitted, not on chain, not audited. Nothing in this repo is live.

## What is implemented

- `script/` - the dip-buy companion (FUEL-era engine, DIP constants in
  `variants/fuel2/`). Buys only when the pool price is 8%+ under its own tracked
  reference, at most one buy per 5 minutes, capped at 1% of the pool's real SOL
  per buy, bought tokens burned. This is the "dip-buy third" of the Dip Vault
  design.
- `harness/` - host test suite T1-T14 for the engine (LiteSVM-based),
  included but not yet executed in this environment. T9, the full LiteSVM run
  of the real `companion_step`, has never been run and is the biggest open
  test gap (needs the Solana toolchain and about 5.4 GB disk).
- `hook/` - the MOON/WICK quote-cut hook. This is the engine reference, NOT the
  Dip Vault hook.
- `tools/` - dip tuning and economics simulators used to set the constants.
- `SPEC.md`, `SUBMIT_DIP.md` - base engine spec and the DIP companion build
  spec/status. Where they and DIP-VAULT.md disagree, DIP-VAULT.md wins.

## What is NOT implemented

The Dip Vault hook core from DIP-VAULT.md does not exist yet:

- per-wallet hold clock (weighted-average entry time)
- 1% sell brake while the hold clock is under 10 minutes
- pot thirds: dip-buy+burn / weekly weighted holder SOL payouts (6h+ hold,
  1x/1.5x/2x weights) / locked liquidity
- builder cut scaling 1-10% with pool liquidity (owner's current decision,
  2026-10-18; exact steps pending the pool-read feasibility check -
  DIP-VAULT.md BUILD ORDER item 3; fallback is flat 10% if hooks cannot read
  pool liquidity)
- locked-LP drip adder

## Verification gates before any submission (from DIP-VAULT.md BUILD ORDER)

1. Confirm holder payouts are buildable as documented (a companion cannot
   enumerate holders; check how Loyalty/Tenure do it).
2. Confirm a companion can add permanently locked liquidity to its own pool.
3. Confirm a hook can read pool liquidity when answering quote_cut (gates the
   1-10% builder scale).
4. Run T9 (LiteSVM, real companion_step) before anything goes live.

## Hygiene

No private keys or seed material: program-id keypairs were never committed
(.gitignore excludes `artifacts/`); this copy also excludes git history, build
output, third-party mirrors (`ref/`), unrelated coin variants, and working
drafts. The builder wallet referenced in configs is a public payout address.
