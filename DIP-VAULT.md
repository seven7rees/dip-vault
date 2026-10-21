# DIP VAULT (v5, 2026-10-18; author-share-0 revision same day) - supersedes v4, TIME-VAULT.md and TIME-VAULT-V3.md

Pitch: "No early-sell tax. Hold 6 hours and get paid." (v5 dropped the 10/6/3% sell ladder. Say "no early-sell tax", not a bare "no sell tax": a 1% brake still applies for 10 minutes and the launcher-set creator fee still applies.)

Time Vault becomes Dip Vault. Where this file and TIME-VAULT.md disagree, this file wins.

## THE OFFER (current parameters - these replace the 7-day ladder in TIME-VAULT.md)

1. Buys: 0% fee.

2. Sells: no early-sell ladder (the v4 10/6/3% ladder is removed, approved by the owner
   2026-10-18). One short anti-dump brake only: 1% (trader-facing) on a sell while the
   seller's hold clock is under 10 minutes. After 10 minutes the hook adds nothing.

   The hook answers net of Bordrless's ~25% markup (trader fee = hook fee x 1.25), so quote
   80 bps for the brake tier and 0 after. Verify the exact markup and its math in the docs
   before hardcoding the bps.

   Hold time is the wallet's hold clock (item 5): a weighted-average entry time, not the
   wallet's first buy. Selling early is not taxed; it just forfeits payout weight (item 5).

3. Fee split: 50% to the launcher, 50% to the pot. SUBMIT WITH AUTHOR SHARE 0
   (authorShareBps 0, owner approved 2026-10-18): the launcher then nets the full 50% part
   of the creator fee - double FUEL's 25% net (FUEL was submitted with a 50% author share,
   so its launchers keep only half of their half). The builder takes no author share; the
   pot cut in item 4 is the builder's only earning. The pot is now funded mainly by the
   launcher-set creator fee (pair config, any quarter percent 0.25-5%; FUEL runs 2%), plus the
   small 1% brake. The ladder no longer fills it. MODELED (not measured; assumes 20% of sells happen in the first 10 minutes): at a 2% creator fee
   the pot gets about 1.04% of volume, versus about 2.13% under the v4 ladder, so roughly
   half the inflow ($104 vs $213 per $10k volume; $624 vs $1,278 per $60k). Sensitivity: a 1%
   creator fee gives $54 per $10k, 3% gives $154. Pot size now depends on the creator fee the
   launcher picks. Open question: confirm how the creator fee applies to buys and sells on
   this hook before claiming "0% buys" next to it in listing copy.

4. The pot: 1% to 10% to the builder (me) - the builder's ONLY earning now that the author
   share is 0 (item 3) - scaling with the coin's liquidity - the deeper the
   pool, the higher the builder cut (exact steps set after the feasibility check in BUILD
   ORDER item 3). Owner chose scaling again on 2026-10-18 (flat 10% from 2026-10-17 is
   replaced). The rest splits in three equal shares, 30/30/30 style: one third buys the dip
   and burns, one third is shared with holders weekly in SOL, one third buys the coin and
   locks it as permanent liquidity.

5. Holder payouts: weekly SOL, no claims needed. Payouts start only after 6 hours of holding;
   a wallet that sells before 6 hours earns nothing for the time held. Weighted by amount held
   and holding age:
   - 1x weight at 6 hours held (nothing before)
   - 1.5x at 7 days
   - 2x at 30 days

   Hold clock (per wallet, used by the 10-minute brake and by payouts): each wallet's clock is a
   weighted-average entry time, not its first buy. Adding coins moves the clock toward now in
   proportion to the amount added (a 30-day wallet that buys 1,000x its balance is treated as
   new; adding 1% barely moves it). Sending coins out does not change the sender's clock.
   Coins arriving in a wallet with no balance start its clock at now (inside the 10-minute brake).
   This closes the top-up loophole (buy fresh into an old wallet, dodge the brake and jump the
   payout wait). The delegate
   loophole is BUILD ORDER item 5. Implementation needs the destination's balance and stamp
   readable in before_transfer; verify both before writing it.

6. Dip buy (companion): buys only when the price is 8% or more under its own tracked average.
   Never more than 1% of the pool's real SOL in one buy (DIP's existing cap, T10). At most one
   buy per 5 minutes. Bought tokens are burned.

7. Locked liquidity (companion): the liquidity third buys the coin and pairs it with SOL as
   permanently locked LP on the coin's own pool, dripped in over about 4 hours per tranche
   (LiquidityTek-style), so one add does not move the price. Reuse FUEL's locked-LP code
   where it fits.

8. Anti-sniping: one line in the listing copy only. OPTIONAL later idea (not in the offer): cap buy
   size in the first few minutes, with no fee on buys. Needs its own verification.
   Bordrless's Bordrless's own launch fee (decaying from
   80% over the first 30 seconds) already covers it. No custom anti-snipe logic.

9. Step pay (unchanged from DIP): whoever sends a companion step is paid up to 0.00002 SOL
   from the pot, even when idle. The flow picture states it; keep it.

## BUILD ORDER AND OPEN VERIFICATIONS

1. FIRST: verify holder payouts are buildable. The v3 spec said direct per-holder SOL payouts
   were not buildable because a companion cannot enumerate holders. But Loyalty Rewards and
   Tenure on the Workshop pay holders, so a supported path likely exists. Check the docs (and
   how those hooks do it) before writing any payout code. This needs a Pair (hook + companion).
   If it is not buildable as documented, stop and tell the owner.
2. NEW: verify the locked-LP adder is buildable. LiquidityTek on the Workshop drips its pot
   into locked LP, and FUEL has locked-LP code to reuse, so a supported path likely exists.
   Confirm in the docs that a companion can add permanently locked liquidity to its own
   launch pool before writing any LP code. If it is not buildable as documented, stop and
   tell the owner.
3. Verify the builder scale is buildable: can a hook read the pool's liquidity when it answers
   quote_cut? If hooks cannot read pool liquidity, fall back to a flat 10% and tell the owner.
   Set the exact liquidity steps after this check.
4. T9 next: the LiteSVM run of the real companion_step has never run and is the biggest open
   risk (needs about 5.4 GB free disk and the Solana toolchain - ask the owner before
   installing it). HANDOFF.md says run it before anything goes live. The first on-chain test
   should also cover the new payout path.
5. Dust-delegate loophole, state-relay fix (unchanged from v3, now smaller): quote_cut gets the
   SIGNER's holding, and token delegates let a seller route a young holding's sale through an
   old dust wallet to skip the 1% brake. With the ladder gone the prize is only 1%, so weigh the
   build cost against it; payouts are not affected. Fix: before_transfer records the real source stamp in the per-mint hook
   state; quote_cut (sell, After phase) reads that record. Confirm the DEX sell path calls the
   token transfer before after_swap's quote_cut in the same transaction, and match the record
   to the pool. Fallback: cut in tokens in before_transfer keyed on the real source stamp;
   pick it only if the relay fails, and say why.
6. Then build the hook and companion. Keep the T-tv1 to T-tv4 tests and add the loophole test
   (route a young holding's sale through an old dust delegate wallet; assert the 1% brake keys on
   the real source stamp, not the signer's) and a payout test (a wallet that sells before 6 hours
   gets no payout weight). The two v3 verify items still stand: (a) does
   quote_cut receive the source holding's hook data; (b) can one hook both write hook data on
   transfers and answer quote_cut. before_transfer must never refuse an ordinary transfer.

## ART

Final art direction: dark magenta-to-orange neon (pink). v5 art (2026-10-18) matches this spec.
- Flow diagram: art/flow/DIPVAULT-flow-dark-v5.png (source art/flow/DIPVAULT-flow-dark.svg,
  generator tools/make_flow_art_dark.py, run from the repo root, then render the SVG at
  1672x941 with headless Chrome). The older art/flow/DIPVAULT-flow-dark.png is the v4 picture
  (10/6/3% ladder) and is out of date; do not use it.
- Square logo: art/logo/dipvault-logo-pink-v5.png ("hold 6 hours, get paid."). The older
  art/logo/dipvault-logo-pink.png is v4 ("sell free") and is out of date.
- Fonts: Space Grotesk first choice; Verdana and DejaVu Sans are the fallbacks.

## RULES THAT STILL APPLY

- Nothing on chain. Nothing submitted. The owner submits.
- Ask the owner before any spend and before installing the Solana toolchain.
- BORDR buyback: dropped. Companions trade only on their own launch pool, and half of protocol
  revenue already buys BORDR, so Dip Vault volume feeds BORDR buybacks automatically.
- Self-reviewed, not audited. Say "not audited" in every listing and post.
- Keep payout wording neutral: holder payouts can read as profit sharing.
- Never call the code audited, safe or guaranteed.
