//! The dip rule: buy only while the pool's price is under this script's own reference price.
//!
//! The companion keeps a reference price too, but for a script launch it is set once at the launch
//! and never moves again, so the script keeps its own (`Ref`, stored in the script state).
//!
//! * The reference starts at the companion's launch price.
//! * A step that buys only ever lowers it, toward the price the buy left.
//! * A step that does not buy raises it toward the price, by at most `ref_step_bps` per step
//!   interval since it last moved, catching up at most `max_ref_steps` moves at once.
//! * A buy needs the price at least `trigger_bps` under the reference and `cooldown_secs` since the
//!   last buy. Its size grows in a straight line with the drop, from 1x at the trigger to
//!   `max_mult`x, and never past the usual caps.
//!
//! Nothing here can panic: arithmetic is u128 and saturating, and a reference of 0 means unset.

use crate::params::Dip;

/// Basis points in a whole.
const BPS: u128 = 10_000;

/// Prices are quote per base unit, times this.
pub const PRICE_SCALE: u128 = 1_000_000_000_000;

/// What the script remembers between steps about the price it measures dips from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ref {
    /// The reference price, `PRICE_SCALE` times quote per base unit; 0 until the first step.
    pub price: u128,
    /// The last time a step moved (or looked at moving) the reference up.
    pub at: i64,
    /// The last time a step bought.
    pub last_buy_at: i64,
}

/// The pool's price: its quote side over its base side, real and virtual, times `PRICE_SCALE`.
/// `None` when the base side is empty.
pub fn spot_price(
    quote_reserve: u64,
    virtual_quote: u64,
    base_reserve: u64,
    virtual_base: u64,
) -> Option<u128> {
    let quote = u128::from(quote_reserve) + u128::from(virtual_quote);
    let base = u128::from(base_reserve) + u128::from(virtual_base);
    (base > 0).then(|| quote.saturating_mul(PRICE_SCALE) / base)
}

/// `reference` moved toward `spot` by at most `step_bps` of it (all the way when unset).
pub fn step_toward(reference: u128, spot: u128, step_bps: u16) -> u128 {
    if reference == 0 {
        return spot;
    }
    let step = reference.saturating_mul(u128::from(step_bps)) / BPS;
    if spot > reference {
        spot.min(reference.saturating_add(step))
    } else {
        spot.max(reference.saturating_sub(step))
    }
}

/// `reference` moved toward `spot` once for every `interval` since `reference_at` (at least once,
/// at most `max_steps`), so one crank after a long quiet catches up.
pub fn catch_up(
    reference: u128,
    spot: u128,
    reference_at: i64,
    interval: i64,
    now: i64,
    step_bps: u16,
    max_steps: u32,
) -> u128 {
    let steps = (now.saturating_sub(reference_at) / interval.max(1)).clamp(1, i64::from(max_steps).max(1));
    let mut r = reference;
    for _ in 0..steps {
        let next = step_toward(r, spot, step_bps);
        if next == r {
            break;
        }
        r = next;
    }
    r
}

/// The first step: an unset reference becomes the launch price, as of the launch time, so a first
/// step that comes late catches the reference up toward the price in one go (at most
/// `max_ref_steps` moves). With no launch price it starts at the pool's price, as of now.
pub fn seed(r: Ref, launch_price: u128, launch_at: i64, spot: u128, now: i64) -> Ref {
    if r.price != 0 {
        return r;
    }
    if launch_price == 0 {
        return Ref { price: spot, at: now, ..r };
    }
    let at = if launch_at > 0 { launch_at.min(now) } else { now };
    Ref { price: launch_price, at, ..r }
}

/// Whether this step may buy, and how big, as `scale_bps` of the base size (10_000 is 1x).
/// `None` when there is no dip: the reference is unset, the price is not `trigger_bps` under it,
/// or the last buy was under `cooldown_secs` ago.
pub fn decide(r: &Ref, spot: u128, now: i64, d: &Dip) -> Option<u64> {
    if r.price == 0 || now < r.last_buy_at.saturating_add(d.cooldown_secs) {
        return None;
    }
    let trigger = u128::from(d.trigger_bps.max(1));
    let threshold = r.price.saturating_mul(BPS.saturating_sub(trigger)) / BPS;
    if spot > threshold {
        return None;
    }
    // How far under the reference, in basis points of it; at least `trigger` here.
    let drop = (r.price - spot.min(r.price)).saturating_mul(BPS) / r.price;
    let top = trigger.saturating_mul(u128::from(d.max_mult.max(1)));
    let scale = drop.min(top).saturating_mul(BPS) / trigger;
    Some(u64::try_from(scale).unwrap_or(u64::MAX))
}

/// After a step that did not buy: once per `step_secs`, raise the reference toward the price.
/// It never lowers here.
pub fn after_wait(r: Ref, spot: u128, now: i64, d: &Dip) -> Ref {
    if now < r.at.saturating_add(d.step_secs) {
        return r;
    }
    let price = if spot > r.price {
        catch_up(r.price, spot, r.at, d.step_secs, now, d.ref_step_bps, d.max_ref_steps)
    } else {
        r.price
    };
    Ref { price, at: now, ..r }
}

/// After a step that bought: the reference only ever lowers, toward the price the buy left (the
/// price a buy leaves is partly the buy itself, so it must never raise the reference).
pub fn after_buy(r: Ref, price_after: Option<u128>, now: i64, d: &Dip) -> Ref {
    let price = match price_after {
        Some(p) => r.price.min(step_toward(r.price, p, d.ref_step_bps)),
        None => r.price,
    };
    Ref { price, last_buy_at: now, ..r }
}
