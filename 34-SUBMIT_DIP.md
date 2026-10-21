> **2026-10-17: DIP is not going to be submitted on its own. The owner merged it with Time Vault into one item (working name "Dip Vault"): see `TIME-VAULT-V3.md`. This file stays as the fallback if the merge is dropped.**

DIP (was FUEL 2) - build spec and status for Claude

Reading this file: DIP in plain text is the Workshop item. `DIP` in code font is the dip-settings struct in `params.rs` (`DIP.trigger_bps` and so on); it exists in every variant and predates the item name.

Repo: seven7rees/moon-wick (private). Read SPEC.md first; its rule applies here too: read the cited source before relying on a fact, and if the source disagrees, the source wins. Say so in your report.

## WHAT THIS IS

A second Workshop submission of the companion kind (script only, no hook), called DIP (name decided 2026-10-17; the internal key in variants/, dist/ and the harness stays `fuel2`). Same engine as FUEL (commit b9e3581, deployed as program `21mthSoY9mw3iqd58wNsohnePoEPCz67j7BXzbxMi4Aa`) with different constants: it buys only on real dips, fewer and bigger, and gives the launcher more.

Pitch: a buyback for your coin that spends only on dips. When the price is 8% under its own mark, the pot buys and burns the coin: 1/48 of what is left, up to 4 times that on a 32% drop. Runs itself every 5 minutes. The launcher keeps half of every creator fee.

STATUS: built, tested on the host, NOT submitted, NOT paid. Owner plan: submit when FUEL's earnings in `AxoN4hpQJWKFkSimhqnA7dUsqf5VGojA5RhhZTiP4KFm` cover about 1.4 SOL (0.1 SOL fee, not refunded, plus about 1.28 SOL deploy; FUEL's quote was 1.281878 SOL).

## TARGET ECONOMICS (and the honest platform mapping)

1. Trade cut 0%: no hook, no sell cut, nothing taken from trades by the script. The trader still pays the platform's fees: at a 2% creator fee about 2.8% a side (2% x 1.25 for Bordrless's quarter, plus 0.3% LP; `harness/tests/t11_min_out.rs` checks this against the real swap math).
2. Fee split. Each creator fee splits by launcherShareBps (5000): half to the launcher directly, half to the companion pot. With "Your share" = **None** at submission there is no author cut:
   - Launcher: 50% of every fee.
   - Pot: 50%. The builder's wallet payout is `payout_bps` 1000 = 10% of new pot money = **5% of total fees**.
   - The other 45% of fees funds the buyback (the buy pile). It sits idle until a dip. Compare FUEL: launcher 25%, builder 25% plus 5%.
3. Dip rule (`script/src/dip.rs`, constants in `variants/fuel2/params.rs`): buy only when spot is at least 8% under the script's own mark; at most once per 300 s; size = 1/48 of the buy pile scaled in a straight line from 1x at 8% to 4x at 32%; never over the pile, never under 0.0005 SOL, never over a share of the pool's real SOL that follows the launch's fees (at most 1%). The mark starts at the launch price (as of launch time), rises toward the price by at most 5% per step, and only a buy lowers it.
4. Idle cost. The companion pays whoever sends a step 20,000 lamports (0.00002 SOL) even when nothing is bought, 2,500 on a step that pays the builder. At a step every 300 s that is about 0.006 SOL a day (FUEL, every 60 s: 0.029).
5. Builder take: 5% of fees plus nothing else, shown in the words ("The builder takes up to 10% of new pot money"). Builder wallet: `WALLET.txt`, same `tools/set_wallet.py` flow.

## THE ONE CHANGE FROM FUEL

Constants only; no script logic changed (`dist/fuel2/script/src` equals FUEL's except `params.rs`):

| | FUEL | DIP | Why |
|---|---|---|---|
| Trigger (`DIP.trigger_bps`) | 300 | 800 | 3% is inside normal memecoin noise; at 1% a minute of wobble FUEL buys about 78 times a day, DIP about 16 |
| Step (`DIP.step_secs`, keeper `stepSecs`) | 60 | 300 | idle bounty 5x lower; the mark rises 5% per 5 minutes, so it lags a fast spike more |
| Buy size (`pace_steps`) | 240 | 48 | fewer buys, so each must be bigger to matter (a 40% crash spends about 29% of the pile) |
| Author share (submit-page choice) | 50% | None | launcher keeps half the fee |

Visible consequences, accept and document or it does not ship: a sharp 20% wick is bought once; a pump then partial dump (price still above its old mark) is not bought at all; the buy cap and the dip rule are unchanged in kind. Everything FUEL's listing says about limits still applies (it buys pullbacks, not bottoms; a launch with a hook or token rules strands the pot).

## HOOK

None. `needs_hook false`, so a launch with any custom hook or rule kit is refused at every step (`WrongHookSetup` / `Unsupported`); the words say "Launch with no hook or token rules".

## SCRIPT

- `variants/fuel2/params.rs`: payout_bps 1_000, pace_steps 48, needs_hook false, min_buy 500_000, max_buy_pool_bps 100, min_out_bps 9_000, bounty_bound 100_000, dip_only true; the `DIP` settings struct { trigger_bps 800, ref_step_bps 500, max_ref_steps 64, max_mult 4, cooldown_secs 300, step_secs 300 }.
- `variants/fuel2/keeper.json`: FUEL's, with `stepSecs` 300 (stateLen 128, payoutTerms stepBps 1000 / holdingBps 10000 / windowBps 5000 / windowSecs 86400, launcherShareBps 5000, mintWritable true).
- `tools/make_variants.py`: `fuel2` entry plus `OVERRIDES` (per-variant `DIP` struct values and keeper step). The tuning script is `tools/sim_dip_tuning.py`.
- Files for the submit page (6, under the 8-file and 120 kB limits): `dist/fuel2/script/src/{lib,dip,layout,math,params}.rs` and `dist/fuel2/script/keeper.json`.

## TESTS (harness, host-only)

- T8 (extended): fuel2 params and keeper agree (trigger 800, step 300, pace 48, stepBps 1000, `DIP.step_secs` equals keeper `stepSecs`).
- T14 `harness/tests/t14_fuel2.rs`: wobbly markets bought far less than FUEL; a 40% crash still bought 5+ times and 15% to 60% of the pile; a one-off wick bought exactly once; a pump and partial dump bought zero times.
- T1-T13 apply unchanged (same code): 121 host tests passed on the Mac before the 2026-10-17 edits to t8 and t14 (comment and message text only; re-run `cargo test --manifest-path harness/Cargo.toml` before paying); `python3 -I tools/scan_rules.py dist` is clean; `cargo build-sbf` of `dist/fuel2/script` builds (198,296 bytes).
- **T9 (a LiteSVM run of the real `companion_step`) has never run.** Two attempts to start it failed (first no disk space, then the session stopped). It stays the biggest open risk and should run before spending real money on DIP.

## SUBMISSION FACTS THAT STILL APPLY (F13-F15)

Max 8 files and 120,000 bytes; name under 32 characters; builder's words 600 characters at most (`variants/fuel2/notes.txt`, 563, grade 2.5), and never "audited", "safe" or "guaranteed" anywhere (F15; the words say "Not audited"). Submission fee 0.1 SOL, non-refundable: do not submit; the owner submits.

GATE before step 1: T9 (the on-chain-style run of the real `companion_step`) has never run. The 0.1 SOL fee and the deploy are not refundable, and Bordrless keeps the upgrade key, so a bug found after deploy costs a new submission and a new fee. Either run T9 on the Mac first, or write the decision to accept that risk here, with the date, before paying. Also re-run the host harness: two test files changed on 2026-10-17 with no cargo run. Money: FUEL had 0 launches as of 2026-10-17, so there are no FUEL earnings yet; submitting now means paying from the owner's own wallet.

Steps for the owner, signed in as `AxoN...4KFm` at https://bordrless.app/workshop/submit, kind **Companion**:
1. Upload the 6 files; name `DIP`; words from `variants/fuel2/notes.txt`; picture `art/dip.png`.
2. **Terms: "Your share" = None.** The words and the picture say the launcher keeps half the fee; with 50% they would be false.
3. After submitting, Edit the builder's words and Change image to `art/flow/DIP-flow.png`. That PNG is not in the repo yet: render it on the Mac first (`python3 -I tools/make_flow_art.py && ./tools/render_art.sh art/flow/DIP-flow.svg`, Marker Felt is a Mac font; the Windows Edge render spilled text), commit it, then upload.
4. Pay the fee, wait for Checks and Approval, then the deploy quote (about 1.28 SOL). In the wallet popup check the payee and the amount. Deploy is permanent; Bordrless keeps the upgrade key, so any change is a new submission and a new fee.

## OPTIONAL - TIME VAULT PAIRING (owner: a maybe, not now)

The Time Vault spec (Gmail draft "Time Vault hook - Claude build spec") proposes reusing FUEL's dip engine in a decaying-sell-cut pair. DIP's constants are the more selective set for that: a one-line change in the timevault params (`dip_only true`, the `DIP` settings struct from `variants/fuel2/params.rs`). Not wired anywhere.

## DELIVERABLES

Done: `dist/fuel2/` ready for the submit page, harness green (121, last run on the Mac before the 2026-10-17 test-file edits; re-run before paying), scan_rules.py clean, picture `art/dip.png` ready; `art/flow/DIP-flow.svg` ready with the DIP title, PNG still to be rendered on the Mac, words in `drafts/notes-v7.txt` ("DIP notes").

Verified against source: the dip rule and every constant (T10, T14, and the three-lens review plus the fresh-eyes pass of the shared code at b9e3581); the fee numbers in the words (1/48, 32%, 8%, 5 min, 0.0005 SOL, 1% cap, 10% payout, 0.00002 SOL idle step); keeper keys against the live docs and approved items; `stepSecs` 300 is within the companion's 10 to 86,400 range.

Not verified: any run on chain (T9); the constants themselves were chosen with a Python simulation on random price paths (`tools/sim_dip_tuning.py`: Gaussian per-minute moves, a plain price path with no pool impact), not live markets; no independent reviewer looked at the new constants specifically (the code is the reviewed FUEL code); that the site accepts "Your share = None" with a script that has wallets and payout terms (live items with None exist, such as Loyalty Rewards and Slow Burn, but none with exactly this keeper); how the site's AI reading will describe it. The dip reference anchor used: `Companion.reference_price` and `reference_at` (bytes 212..236), the launch price and launch time.

Nothing goes on chain. Nothing gets submitted.
