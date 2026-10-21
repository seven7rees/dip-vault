# Dip Vault (Bordrless hook + companion) - private handoff repo

Status: **design and spec final (v5.1), art final. Implementation not built yet - no code in this repo.**
This repo is the scoped handoff for the Bordrless team. It holds only Dip Vault material.

- `DIP-VAULT.md` - the full spec (v5.1). This is the design contract.
- `LISTING.md` - the planned Workshop listing text, matching the spec.
- `art/flow/DIPVAULT-flow-dark-v5.png` - wide flow graphic (how the fees and pot work).
- `art/logo/dipvault-logo-pink-v6.png` - square logo / page image.

Pitch: "No early-sell tax. Hold 6 hours and get paid." The hook's only sell fee is 1% for the
first 10 minutes (anti-dump brake); buys pay no hook cut. Half the creator fee goes to the
launcher in full (author share 0, no skim to the builder); half fills a pot that buys dips and
burns, pays holders SOL weekly, and locks liquidity. Builder's only earning is 1-10% of the pot.

Self-reviewed, not audited. Nothing is promised. Payout and liquidity-lock behavior described in
the spec is the design, not deployed code.
