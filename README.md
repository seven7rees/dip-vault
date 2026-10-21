# Dip Vault - a Bordrless Workshop pair (private review copy)

Status 2026-10-21: design DIP-VAULT.md v5 with two owner overrides (no burn anywhere; the builder
cut scales 1-10% with pool SOL). Hook and script IMPLEMENTED and COMPILED for Solana (cargo
build-sbf); 165 host tests TEST-EXECUTED; T9, the whole story in LiteSVM, TEST-EXECUTED against the
programs built from the pinned source and against the live mainnet program bytes. Three
independent reviews; their findings and status are in REVIEW2-dipvault-2026-10-21.md,
REVIEW3-package-2026-10-21.md and REVIEW3-status-2026-10-21.md. NOT submitted, NOT deployed, not
audited. The full build report: DIP-VAULT-BUILD.md. Words for the graphic and the video:
DIP-VAULT-FORMAT-NOTE.txt.

## What is here

- `hook_variants/dipvault/` - the hook (hold clock in every holding, 1% brake for the first 10
  minutes of a hold, the sell relay against the dust-delegate trick, `record_payout` for the
  companion). `hook.json` beside it.
- `script_variants/dipvault/` - the companion script (the pot in four piles: builder 1-10% by
  pool SOL, dip buys that keep the coins, weekly weighted holder payouts, locked liquidity adds
  pairing the coins held). `keeper.json` beside it.
- `harness/tests/t15..t19` - host tests of every rule (run from the moon-wick checkout: the
  harness needs `ref/` and the engine crates; `tools/fetch_refs.sh` recreates `ref/`).
- `litesvm/` - T9.
- `variants/dipvault/notes.txt` - the 600-character listing words.
- `WALLET.txt` - the builder's PUBLIC payout address only.

## Verification gates before any submission

Done: the three feasibility checks (DIP-VAULT-BUILD.md); T9 on local builds and on mainnet bytes.
Not done and not doable offline: the Workshop's own rule scan, build, trading test and reading.
Open owner decisions: the builder scale steps, the never-sold bonus, the one-holder-a-step
weekly reach, the two fixed registry keys (LiquidityTek precedent).

## Hygiene

No private keys or seed material. `declare_id!` holds local build addresses (public); Bordrless
writes the real ids at submission. `ref/`, `target*/` and keypair files are ignored.
