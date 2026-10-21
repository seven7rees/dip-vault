//! Pure functions for the pot split, buy sizing, the payout budget and the answers.
//!
//! Nothing here can panic, whatever the inputs: arithmetic goes through u128, and anything that would
//! not fit a u64 is capped instead of wrapping.

use anchor_lang::prelude::Pubkey;

/// Basis points in a whole.
const BPS_DENOMINATOR: u128 = 10_000;

/// `floor(x * b / 10_000)` for a u16 rate; capped at `u64::MAX`.
pub fn bps128(x: u64, b: u16) -> u64 {
    let scaled = u128::from(x) * u128::from(b) / BPS_DENOMINATOR;
    u64::try_from(scaled).unwrap_or(u64::MAX)
}

/// `floor(x * b / 10_000)` for a u64 rate; capped at `u64::MAX`.
pub fn bps64(x: u64, b: u64) -> u64 {
    let scaled = u128::from(x) * u128::from(b) / BPS_DENOMINATOR;
    u64::try_from(scaled).unwrap_or(u64::MAX)
}

/// The first second of the payout window that `now` falls in:
/// `floor(now / max(window_secs, 1)) * max(window_secs, 1)`. `div_euclid` keeps a negative clock
/// on the right side of the boundary.
pub fn window_start_now(now: i64, window_secs: u32) -> i64 {
    let secs = i64::from(window_secs.max(1));
    now.div_euclid(secs).saturating_mul(secs)
}

/// The pot a window's share is taken of: the pot after this step's spending plus what the window has
/// already paid, and never less than the pot at the window's first step.
pub fn window_base(pot_after: u64, w_paid: u64, w_pot: u64) -> u64 {
    pot_after.saturating_add(w_paid).max(w_pot)
}

/// The most this step's payout answer may add up to. The companion computes the same number and
/// refuses a whole step whose answer is above it, so this must match its rule exactly:
///
/// ```text
/// budget = min(
///     bps(pot_before,  step_bps),
///     bps(window_base, window_bps) - w_paid   (floor at 0),
///     pot_after - bounty                      (floor at 0),
/// )
/// ```
///
/// `bounty` is an upper bound on what the companion pays the step's sender, so the result is never
/// above the companion's own budget. `w_pot` and `w_paid` are the window counters after the window
/// roll (a new window starts at the pot after this step, with nothing paid).
pub fn step_budget(
    pot_before: u64,
    pot_after: u64,
    bounty: u64,
    w_pot: u64,
    w_paid: u64,
    step_bps: u16,
    window_bps: u16,
) -> u64 {
    let base = window_base(pot_after, w_paid, w_pot);
    let by_step = bps128(pot_before, step_bps);
    let by_window = bps128(base, window_bps).saturating_sub(w_paid);
    let by_pot = pot_after.saturating_sub(bounty);
    by_step.min(by_window).min(by_pot)
}

/// What the companion pays a step's sender out of the pot once the script has run: the flat
/// `SCRIPT_BOUNTY` (20_000) when the step answers nothing, `SCRIPT_BOUNTY * min(n, 8) / 8` for an
/// answer of `n` entries, and nothing at all when the pot after the script does not cover it. The
/// script needs the exact figure to remember the pot as the companion leaves it.
pub fn step_bounty(pot_after_script: u64, answer_entries: Option<u64>) -> u64 {
    let wanted = match answer_entries {
        None => COMPANION_SCRIPT_BOUNTY,
        Some(n) => COMPANION_SCRIPT_BOUNTY * n.min(COMPANION_PAYOUT_BOUNTY_FULL) / COMPANION_PAYOUT_BOUNTY_FULL,
    };
    if pot_after_script >= wanted {
        wanted
    } else {
        0
    }
}

/// The pot as the companion leaves it after the step, which is what the script remembers as
/// `last_pot`: the pot after the script ran, less what the step paid the builder and the sender
/// bounty the companion takes afterwards (a step that pays the builder answers one entry).
pub fn pot_after_step(pot_after_script: u64, pay: u64) -> u64 {
    let bounty = step_bounty(pot_after_script, if pay > 0 { Some(1) } else { None });
    pot_after_script.saturating_sub(pay).saturating_sub(bounty)
}

/// The companion's flat step bounty, in lamports (`SCRIPT_BOUNTY`).
pub const COMPANION_SCRIPT_BOUNTY: u64 = 20_000;
/// An answer of this many entries earns the whole bounty (`PAYOUT_BOUNTY_FULL`).
pub const COMPANION_PAYOUT_BOUNTY_FULL: u64 = 8;

/// Size one buy from the buy pile.
///
/// ```text
/// raw    = pile / pace_steps
/// amount = max(raw, min_buy)                          (pile must be at least min_buy)
/// amount = min(amount, pile, quote_reserve * max_buy_pool_bps / 10_000)
/// ```
///
/// Returns `None` when no buy should happen: the pile is below `min_buy`, or the pool's real quote
/// reserve is so small that the pool cap leaves less than `min_buy`. `quote_reserve` is the pool's
/// real SOL only, not the virtual offset: the cap is there so one buy stays too small to be worth
/// a sandwich, and virtual SOL is not liquidity anyone can take out. The caller passes the cap as
/// `pool_share_bps` of the launch's fees, so a launch with low fees gets a smaller one.
pub fn buy_amount(
    pile: u64,
    pace_steps: u32,
    min_buy: u64,
    max_buy_pool_bps: u16,
    quote_reserve: u64,
) -> Option<u64> {
    buy_amount_scaled(pile, pace_steps, SCALE_ONE_BPS, min_buy, max_buy_pool_bps, quote_reserve)
}

/// The biggest buy as a share of the pool's real SOL for a launch with these fees: a quarter of the
/// buy and sell fees added up, never over `cap`. Someone who pumps the price by `p` before a buy of
/// `v` and sells after gains about `p * v` and pays about `p * Q * (buy + sell) / 2` in fees (`Q`
/// the pool's SOL), so at half that bound the sandwich always costs more than it makes. This is the
/// companion's own `steps::pool_share_bps`, so a launch with low fees gets a smaller buy cap.
pub fn pool_share_bps(
    lp_fee_bps: u16,
    creator_fee_bps: u16,
    holder_fee_buy_bps: u16,
    burn_buy_bps: u16,
    holder_fee_sell_bps: u16,
    burn_sell_bps: u16,
    cap: u16,
) -> u16 {
    let side = u64::from(lp_fee_bps) + u64::from(creator_fee_bps);
    let buy = side + u64::from(holder_fee_buy_bps) + u64::from(burn_buy_bps);
    let sell = side + u64::from(holder_fee_sell_bps) + u64::from(burn_sell_bps);
    u16::try_from(((buy + sell) / 4).min(u64::from(cap))).unwrap_or(cap)
}

/// One whole in `buy_amount_scaled`'s `scale_bps`: a buy of exactly `pile / pace_steps`.
pub const SCALE_ONE_BPS: u64 = 10_000;

/// [`buy_amount`] with the base size `pile / pace_steps` multiplied by `scale_bps / 10_000` before
/// the same floor and caps. The caps (the pile, the pool's share, `min_buy`) apply after the
/// scale, so no scale can push a buy past them.
pub fn buy_amount_scaled(
    pile: u64,
    pace_steps: u32,
    scale_bps: u64,
    min_buy: u64,
    max_buy_pool_bps: u16,
    quote_reserve: u64,
) -> Option<u64> {
    if pile < min_buy {
        return None;
    }
    let base = u128::from(pile / u64::from(pace_steps.max(1)));
    let scaled = base.saturating_mul(u128::from(scale_bps)) / u128::from(SCALE_ONE_BPS);
    let raw = u64::try_from(scaled).unwrap_or(u64::MAX);
    let amount = raw.max(min_buy).min(pile);
    let amount = amount.min(bps128(quote_reserve, max_buy_pool_bps));
    if amount < min_buy {
        None
    } else {
        Some(amount)
    }
}

/// The least number of tokens a buy of `amount` lamports may deliver.
///
/// The fee-free constant-product output on the pool's effective reserves (real plus virtual),
/// `base_eff * amount / (quote_eff + amount)`, times `min_out_bps / 10_000`, and at least 1.
/// An overflow caps the result at `u64::MAX` instead of wrapping.
///
/// Why 9_000 (10% slack) is enough for an ordinary buy. The swap takes its fees off the input before
/// the curve: the creator fee plus Bordrless's quarter on top of it (`1.25 * creator`), then the LP
/// fee. So the buy delivers the curve's output for `n = amount * (1 - 1.25 * creator) * (1 - lp)`.
/// The curve is concave, so `out(n) >= out(amount) * n / amount`: the buy delivers at least
/// `(1 - fee)` of the fee-free output, with `fee = 1 - (1 - 1.25 * creator) * (1 - lp)`. At the
/// 2% creator fee and 0.3% LP fee that is about 2.8%; at 5% it is about 6.6%; and the guard holds
/// up to a creator fee of about 7.7% (about 7.6% if the pool also took a 0.25% protocol fee off
/// the input of a buy).
/// Past 10% the swap fails with its slippage error and the step is refused with nothing spent.
/// Neither case can come up on a launch made through the site: its creator fee is capped at 2%
/// (5% in a pair's config), and the first step runs a minute after the launch, after the 30-second
/// sniper fee has fallen back. A creator fee of 7.8% or more would not fall back: every buy would
/// be refused for good.
pub fn min_out(
    amount: u64,
    base_reserve: u64,
    virtual_base: u64,
    quote_reserve: u64,
    virtual_quote: u64,
    min_out_bps: u16,
) -> u64 {
    let base_eff = u128::from(base_reserve) + u128::from(virtual_base);
    let quote_eff = u128::from(quote_reserve) + u128::from(virtual_quote);
    let denom = quote_eff + u128::from(amount);
    if denom == 0 {
        return 1;
    }
    let ideal = base_eff
        .checked_mul(u128::from(amount))
        .map_or(u128::MAX, |p| p / denom);
    let out = ideal
        .checked_mul(u128::from(min_out_bps))
        .map_or(u128::MAX, |p| p / BPS_DENOMINATOR);
    u64::try_from(out).unwrap_or(u64::MAX).max(1)
}

/// A wallet payout answer with one entry, as return data:
/// `version u8 = 2 | entries u32 LE = 1 | to Pubkey | lamports u64 LE | tag u32 LE` (49 bytes).
pub fn encode_wallet_answer(to: &Pubkey, lamports: u64, tag: u32) -> [u8; 49] {
    let mut buf = [0u8; 49];
    buf[0] = 2;
    buf[1..5].copy_from_slice(&1u32.to_le_bytes());
    buf[5..37].copy_from_slice(to.as_ref());
    buf[37..45].copy_from_slice(&lamports.to_le_bytes());
    buf[45..49].copy_from_slice(&tag.to_le_bytes());
    buf
}

// ─── Dip Vault: the split of the pot ──────────────────────────────────────────

use crate::params::{BuilderStep, BUILDER_SCALE, DIP_THIRD_BPS, HOLDER_THIRD_BPS};

/// The builder's share of new pot money, in basis points, by the pool's real SOL: the highest
/// step of `BUILDER_SCALE` whose start `quote_reserve` reaches.
pub fn builder_bps(quote_reserve: u64) -> u16 {
    let mut bps = BUILDER_SCALE[0].bps;
    for BuilderStep { from_lamports, bps: b } in BUILDER_SCALE.iter() {
        if quote_reserve >= *from_lamports {
            bps = *b;
        }
    }
    bps
}

/// The four piles the pot is kept in. Their sum is always the pot as the companion left it after
/// the last step (`ScriptState.last_pot`): `owed` to the builder, `dip` for dip buys, `holders`
/// for the weekly payouts, `lp` for the locked liquidity.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Piles {
    pub owed: u64,
    pub dip: u64,
    pub holders: u64,
    pub lp: u64,
}

impl Piles {
    /// The four added up (saturating).
    pub fn total(&self) -> u64 {
        self.owed
            .saturating_add(self.dip)
            .saturating_add(self.holders)
            .saturating_add(self.lp)
    }

    /// `new_money` split: `builder_bps` of it to the builder, the rest in three parts: the dip
    /// third, the holder third, and the liquidity third taking what rounding leaves.
    pub fn split(&mut self, new_money: u64, builder_bps: u16) {
        let builder = bps128(new_money, builder_bps);
        let rest = new_money.saturating_sub(builder);
        let dip = bps128(rest, DIP_THIRD_BPS);
        let holders = bps128(rest, HOLDER_THIRD_BPS);
        let lp = rest.saturating_sub(dip).saturating_sub(holders);
        self.owed = self.owed.saturating_add(builder);
        self.dip = self.dip.saturating_add(dip);
        self.holders = self.holders.saturating_add(holders);
        self.lp = self.lp.saturating_add(lp);
    }

    /// `amount` taken from the piles in the order dip, lp, holders, owed (each as far as it goes):
    /// what the sender's bounty costs, or a pot shortfall. Returns what could not be taken.
    pub fn take(&mut self, amount: u64) -> u64 {
        let mut left = amount;
        for pile in [&mut self.dip, &mut self.lp, &mut self.holders, &mut self.owed] {
            let t = (*pile).min(left);
            *pile -= t;
            left -= t;
        }
        left
    }

    /// The piles made to add up to `pot` exactly: a surplus goes to the dip pile, a shortfall
    /// comes out of the piles in `take`'s order. After this `total() == pot` unless the piles were
    /// all empty and `pot` was 0 already.
    pub fn reconcile(&mut self, pot: u64) {
        let total = self.total();
        if pot > total {
            self.dip = self.dip.saturating_add(pot - total);
        } else if total > pot {
            self.take(total - pot);
        }
    }
}

/// The lamports one liquidity tranche may spend: `pile / pace_steps`, at least `min_add`, never
/// over the pile, and never over twice the pool cap (the buy half is held to the cap a dip buy
/// has, `cap_lamports`, and the pairing half is about as much). `None` below `min_add`.
pub fn lp_tranche(pile: u64, pace_steps: u32, min_add: u64, cap_lamports: u64) -> Option<u64> {
    if pile < min_add {
        return None;
    }
    let raw = pile / u64::from(pace_steps.max(1));
    let amount = raw.max(min_add).min(pile).min(cap_lamports.saturating_mul(2));
    (amount >= min_add).then_some(amount)
}

/// A payout answer naming holdings, as return data: `version u8 = 1 | entries u32 LE | each entry
/// account u8, lamports u64 LE, tag u32 LE` (1 + 4 + 13 per entry).
pub fn encode_payout_answer(entries: &[(u8, u64, u32)]) -> Vec<u8> {
    let mut out = Vec::with_capacity(5 + 13 * entries.len());
    out.push(1u8);
    out.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    for (account, lamports, tag) in entries {
        out.push(*account);
        out.extend_from_slice(&lamports.to_le_bytes());
        out.extend_from_slice(&tag.to_le_bytes());
    }
    out
}

/// `whole * part / of`, rounded down, in u128; 0 when `of` is 0; capped at `u64::MAX`.
pub fn share(whole: u64, part: u128, of: u128) -> u64 {
    if of == 0 {
        return 0;
    }
    let s = u128::from(whole).saturating_mul(part) / of;
    u64::try_from(s).unwrap_or(u64::MAX)
}

/// `a * b / d`, rounded up, in u128; `None` when `d` is 0 or the result does not fit a u64.
pub fn ceil_ratio(a: u64, b: u64, d: u64) -> Option<u64> {
    if d == 0 {
        return None;
    }
    let n = u128::from(a) * u128::from(b);
    let d = u128::from(d);
    u64::try_from(n / d + u128::from(n % d != 0)).ok()
}
