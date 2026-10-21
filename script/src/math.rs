//! Pure functions for buy sizing, the payout budget and the payout answer.
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

/// The buy pile: the pot at the step's start less what is owed to the builder and the builder's
/// share of money parked while payouts were off, so a step that omits the payout accounts can delay
/// the builder but never get that money spent on buys.
pub fn buy_pile(pot_before: u64, owed: u64, unsplit: u64, payout_bps: u16) -> u64 {
    pot_before.saturating_sub(owed.saturating_add(bps128(unsplit, payout_bps)))
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

/// Moves this step's new pot money into the builder's tally.
///
/// With payouts on, `payout_bps` of the new money plus anything parked earlier becomes owed to the
/// builder, and nothing stays parked. With payouts off (no terms account in the step, or wallet
/// payouts not allowed) the new money is parked in `unsplit` instead, so leaving the terms out of
/// one step cannot erase the builder's share of it. Returns `(owed, unsplit)`.
pub fn accrue(owed: u64, unsplit: u64, new_money: u64, payouts_on: bool, payout_bps: u16) -> (u64, u64) {
    if payouts_on {
        let to_split = new_money.saturating_add(unsplit);
        (owed.saturating_add(bps128(to_split, payout_bps)), 0)
    } else {
        (owed, unsplit.saturating_add(new_money))
    }
}

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
