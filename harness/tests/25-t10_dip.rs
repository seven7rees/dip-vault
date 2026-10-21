//! T10 — the dip rule (`dip.rs`) and the scaled buy size.
//!
//! The rule is pure functions, so every property here runs on the real code. `simulate_step`
//! repeats the order of `companion_step` (seed, decide, size, then lower or raise the reference);
//! the account-level path is covered by the on-chain-style test T9, not here.
//!
//! Differential parts check the copies against the programs crate: `spot_price` and `step_toward`
//! against the real public ones, `catch_up` (private upstream, steps.rs) against a model that is
//! its body, built on the real `step_toward` and the real constants.

use bordrless_companion::constants::{MAX_REFERENCE_STEPS, PRICE_SCALE as REAL_PRICE_SCALE, REFERENCE_STEP_BPS};
use bordrless_companion::state::{spot_price as real_spot_price, step_toward as real_step_toward};
use workshop_script::dip::{after_buy, after_wait, catch_up, decide, seed, spot_price, step_toward, Ref, PRICE_SCALE};
use workshop_script::math::{buy_amount_scaled, SCALE_ONE_BPS};
use workshop_script::params::{Dip, DIP};

struct Xorshift64(u64);
impl Xorshift64 {
    fn new(seed: u64) -> Self {
        Self(seed | 1)
    }
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

const MIN_BUY: u64 = 500_000;
const POOL_BPS: u16 = 100;
const PACE: u32 = 240;
const STEP: i64 = 60;

fn d() -> Dip {
    Dip { ..DIP }
}

/// A reference that has been set at `price` and last bought at `last_buy_at`.
fn r(price: u128, last_buy_at: i64) -> Ref {
    Ref { price, at: 0, last_buy_at }
}

fn under(reference: u128, bps: u128) -> u128 {
    reference * (10_000 - bps) / 10_000
}

// ─── constants stay in step with the real companion ──────────────────────────

#[test]
fn constants_match_the_real_companion() {
    assert_eq!(PRICE_SCALE, REAL_PRICE_SCALE);
    assert_eq!(u64::from(DIP.ref_step_bps), REFERENCE_STEP_BPS);
    assert_eq!(i64::from(DIP.max_ref_steps), MAX_REFERENCE_STEPS);
}

// ─── trigger ─────────────────────────────────────────────────────────────────

#[test]
fn no_buy_when_price_is_above_the_trigger() {
    let reference = 1_000_000_000_000u128;
    let threshold = under(reference, 300);
    for spot in [reference * 2, reference, reference - 1, threshold + 1] {
        assert_eq!(decide(&r(reference, 0), spot, 10_000, &d()), None, "spot {spot}");
    }
}

#[test]
fn buys_at_and_below_the_trigger() {
    let reference = 1_000_000_000_000u128;
    let threshold = under(reference, 300);
    assert_eq!(decide(&r(reference, 0), threshold, 10_000, &d()), Some(SCALE_ONE_BPS));
    assert!(decide(&r(reference, 0), threshold - 1, 10_000, &d()).is_some());
    assert!(decide(&r(reference, 0), 1, 10_000, &d()).is_some());
}

#[test]
fn unset_reference_never_buys() {
    assert_eq!(decide(&r(0, 0), 0, 10_000, &d()), None);
    assert_eq!(decide(&r(0, 0), 5, 10_000, &d()), None);
}

// ─── size: a straight line, capped, never doubling ───────────────────────────

#[test]
fn scale_is_a_straight_line_capped_at_max_mult() {
    let reference = 10_000_000_000_000u128; // 10_000 units of 1e9 so basis points are exact
    let at = |drop_bps: u128| decide(&r(reference, 0), reference - reference * drop_bps / 10_000, 10_000, &d());
    assert_eq!(at(300), Some(10_000), "1x at the trigger");
    assert_eq!(at(600), Some(20_000), "2x at twice the trigger");
    assert_eq!(at(900), Some(30_000), "3x");
    assert_eq!(at(1_200), Some(40_000), "4x at max_mult triggers");
    assert_eq!(at(5_000), Some(40_000), "capped at max_mult");
    assert_eq!(at(10_000), Some(40_000), "capped when the price is zero");
}

#[test]
fn a_deeper_drop_never_buys_less() {
    let reference = 10_000_000_000_000u128;
    let mut last = 0u64;
    for drop in 300..=10_000u128 {
        let spot = reference - reference * drop / 10_000;
        let scale = decide(&r(reference, 0), spot, 10_000, &d()).expect("dip");
        assert!(scale >= last, "scale fell from {last} to {scale} at drop {drop}");
        let amount = buy_amount_scaled(5_000_000_000, PACE, scale, MIN_BUY, POOL_BPS, 1_000_000_000_000).unwrap();
        assert!(amount >= 5_000_000_000 / u64::from(PACE), "never under the base size on a dip");
        last = scale;
    }
}

#[test]
fn buy_amounts_never_break_the_caps() {
    let mut rng = Xorshift64::new(0xD1B);
    for _ in 0..100_000 {
        let pile = rng.next() >> rng.below(40);
        let quote = rng.next() >> rng.below(40);
        let scale = rng.below(60_000);
        if let Some(a) = buy_amount_scaled(pile, PACE, scale, MIN_BUY, POOL_BPS, quote) {
            let cap = (u128::from(quote) * u128::from(POOL_BPS) / 10_000) as u64;
            assert!(a <= pile, "over the pile");
            assert!(a <= cap, "over 1% of the pool's real SOL");
            assert!(a >= MIN_BUY, "under min_buy");
        } else {
            let cap = (u128::from(quote) * u128::from(POOL_BPS) / 10_000) as u64;
            assert!(pile < MIN_BUY || cap < MIN_BUY, "refused a buy that fit: pile {pile} cap {cap}");
        }
    }
}

#[test]
fn scale_one_is_the_plain_pace() {
    let a = buy_amount_scaled(2_400_000_000, 240, SCALE_ONE_BPS, MIN_BUY, POOL_BPS, 10_000_000_000_000).unwrap();
    assert_eq!(a, 10_000_000, "pile / 240");
    let b = buy_amount_scaled(2_400_000_000, 240, 4 * SCALE_ONE_BPS, MIN_BUY, POOL_BPS, 10_000_000_000_000).unwrap();
    assert_eq!(b, 40_000_000, "4x");
    // The pool cap still wins over the scale.
    let c = buy_amount_scaled(2_400_000_000, 240, 4 * SCALE_ONE_BPS, MIN_BUY, POOL_BPS, 1_000_000_000).unwrap();
    assert_eq!(c, 10_000_000, "1% of 1 SOL");
}

// ─── cooldown ────────────────────────────────────────────────────────────────

#[test]
fn cooldown_blocks_a_second_buy() {
    let reference = 1_000_000_000_000u128;
    let spot = under(reference, 1_000);
    let t = 1_000i64;
    let after = after_buy(r(reference, 0), Some(spot), t, &d());
    assert_eq!(after.last_buy_at, t);
    // The reference came down a little, but the price is still deep under it.
    assert!(decide(&after, spot / 2, t + 299, &d()).is_none(), "inside the cooldown");
    assert!(decide(&after, spot / 2, t + 300, &d()).is_some(), "after the cooldown");
}

// ─── the reference ───────────────────────────────────────────────────────────

#[test]
fn seed_uses_the_launch_price_then_the_pool_price() {
    // The launch price, as of the launch time.
    assert_eq!(seed(Ref::default(), 777, 40, 5, 100), Ref { price: 777, at: 40, last_buy_at: 0 });
    // A launch time in the future (or unset) falls back to now.
    assert_eq!(seed(Ref::default(), 777, 500, 5, 100).at, 100);
    assert_eq!(seed(Ref::default(), 777, 0, 5, 100).at, 100);
    // No launch price: the pool's price, as of now.
    assert_eq!(seed(Ref::default(), 0, 40, 5, 100), Ref { price: 5, at: 100, last_buy_at: 0 });
    assert_eq!(seed(Ref::default(), 0, 40, 0, 100).price, 0, "nothing to seed from stays unset");
    let set = Ref { price: 9, at: 3, last_buy_at: 4 };
    assert_eq!(seed(set, 777, 40, 5, 100), set, "a set reference is left alone");
}

#[test]
fn a_late_first_step_catches_the_reference_up() {
    // The coin launched at price 1 and sits at 10 when the first step arrives two hours later. The
    // reference starts at the launch price as of the launch, so one step catches it up by up to
    // 64 moves instead of waiting ln(10)/ln(1.05) = 47 minutes for it.
    let launch = 1_000_000_000_000u128;
    let spot = launch * 10;
    let now = 7_200i64;
    let seeded = seed(Ref::default(), launch, 0 + 1, spot, now);
    assert_eq!(seeded.at, 1);
    assert!(decide(&seeded, spot, now, &d()).is_none(), "price is far above the reference: no buy");
    let after = after_wait(seeded, spot, now, &d());
    assert!(after.price >= spot * 99 / 100, "one step caught up to the price: {} vs {spot}", after.price);
}

#[test]
fn a_wait_raises_at_most_five_percent_per_interval() {
    let reference = 1_000_000_000_000u128;
    let spot = reference * 10;
    // One interval since the last move: one move.
    let one = after_wait(Ref { price: reference, at: 0, last_buy_at: 0 }, spot, STEP, &d());
    assert_eq!(one.price, reference * 105 / 100);
    assert_eq!(one.at, STEP);
    // Before an interval has passed: nothing moves.
    let none = after_wait(Ref { price: reference, at: 0, last_buy_at: 0 }, spot, STEP - 1, &d());
    assert_eq!(none.price, reference);
    assert_eq!(none.at, 0);
}

#[test]
fn catch_up_is_capped_at_max_ref_steps() {
    let reference = 1_000_000_000_000u128;
    let spot = u128::MAX / 4;
    let after = after_wait(Ref { price: reference, at: 0, last_buy_at: 0 }, spot, 10_000_000, &d());
    let mut model = reference;
    for _ in 0..64 {
        model = model + model * 500 / 10_000;
    }
    assert_eq!(after.price, model, "64 moves of 5% and no more");
}

#[test]
fn a_wait_never_lowers_and_a_buy_never_raises() {
    let mut rng = Xorshift64::new(0xFA11);
    for _ in 0..100_000 {
        let price = u128::from(rng.next() >> 8) + 1;
        let spot = u128::from(rng.next() >> rng.below(60));
        let at = rng.below(1_000_000) as i64;
        let now = at + rng.below(10_000_000) as i64;
        let start = Ref { price, at, last_buy_at: 0 };
        let waited = after_wait(start, spot, now, &d());
        assert!(waited.price >= price, "a wait lowered the reference");
        let bought = after_buy(start, Some(spot), now, &d());
        assert!(bought.price <= price, "a buy raised the reference");
        // ...toward the price the buy left, never past it, and by at most 5% of itself.
        assert!(bought.price >= price.min(spot).max(price - price * 500 / 10_000));
        assert_eq!(bought.last_buy_at, now);
    }
}

// ─── simulations ─────────────────────────────────────────────────────────────

/// One step of `companion_step`'s dip path against a price. Returns the lamports spent.
fn simulate_step(state: &mut Ref, pile: u64, quote_real: u64, spot: u128, price_after: Option<u128>, now: i64) -> u64 {
    *state = seed(*state, 0, 0, spot, now);
    let amount = decide(state, spot, now, &d())
        .and_then(|scale| buy_amount_scaled(pile, PACE, scale, MIN_BUY, POOL_BPS, quote_real));
    match amount {
        Some(a) => {
            *state = after_buy(*state, price_after.or(Some(spot)), now, &d());
            a
        }
        None => {
            *state = after_wait(*state, spot, now, &d());
            0
        }
    }
}

#[test]
fn a_day_of_green_candles_buys_nothing() {
    let pile = 50_000_000_000u64;
    let mut state = Ref::default();
    let mut spot = 1_000_000_000_000u128;
    let mut spent = 0u64;
    for minute in 0..1_440i64 {
        spent += simulate_step(&mut state, pile, 500_000_000_000, spot, None, minute * STEP);
        spot = spot * 101 / 100; // +1% a minute
    }
    assert_eq!(spent, 0, "no buys while the price rises");
    assert!(state.price > 0);
}

#[test]
fn a_death_spiral_stays_inside_the_caps() {
    let mut pile = 100_000_000_000u64; // 100 SOL
    let start_pile = pile;
    let mut state = Ref::default();
    let mut spot = 1_000_000_000_000u128;
    let mut quote = 500_000_000_000u64; // the pool's real SOL falls with the price
    let (mut spent_total, mut cap_total, mut buys) = (0u64, 0u64, 0u64);
    for i in 0..50i64 {
        let cap = (u128::from(quote) * u128::from(POOL_BPS) / 10_000) as u64;
        let spent = simulate_step(&mut state, pile, quote, spot, None, i * STEP);
        assert!(spent == 0 || (spent <= pile && spent <= cap), "step {i} spent {spent}");
        if spent > 0 {
            buys += 1;
            cap_total += cap;
        }
        pile = pile.checked_sub(spent).expect("the pot went negative");
        spent_total += spent;
        spot = spot * 9 / 10; // -10% a step
        quote = quote * 9 / 10;
    }
    assert!(buys <= 10, "cooldown allows at most one buy per five steps, got {buys}");
    assert!(spent_total <= cap_total, "total spent {spent_total} over the caps {cap_total}");
    assert_eq!(pile, start_pile - spent_total);
}

// ─── edge cases ──────────────────────────────────────────────────────────────

#[test]
fn edge_values_do_not_panic() {
    assert_eq!(spot_price(0, 0, 0, 0), None, "empty pool");
    assert_eq!(spot_price(u64::MAX, u64::MAX, 0, 0), None);
    assert!(spot_price(u64::MAX, u64::MAX, 1, 0).is_some());
    assert!(spot_price(u64::MAX, u64::MAX, u64::MAX, u64::MAX).is_some());
    assert_eq!(spot_price(0, 0, 5, 5), Some(0));

    for reference in [0u128, 1, u128::MAX / 2, u128::MAX] {
        for spot in [0u128, 1, u128::MAX / 2, u128::MAX] {
            let _ = step_toward(reference, spot, 500);
            let _ = decide(&r(reference, i64::MAX), spot, i64::MIN, &d());
            let _ = decide(&r(reference, i64::MIN), spot, i64::MAX, &d());
            let _ = after_wait(Ref { price: reference, at: i64::MIN, last_buy_at: 0 }, spot, i64::MAX, &d());
            let _ = after_buy(r(reference, 0), Some(spot), i64::MAX, &d());
            let _ = catch_up(reference, spot, i64::MIN, 0, i64::MAX, 500, u32::MAX);
        }
    }
    assert!(buy_amount_scaled(u64::MAX, 0, u64::MAX, 0, u16::MAX, u64::MAX).is_some());
    assert_eq!(buy_amount_scaled(u64::MAX, u32::MAX, 0, MIN_BUY, POOL_BPS, 0), None, "empty pool buys nothing");
    // A zero trigger and a zero max multiple are treated as 1.
    let weird = Dip { trigger_bps: 0, max_mult: 0, ..DIP };
    let _ = decide(&r(1_000, 0), 0, 0, &weird);
}

// ─── differential against the real companion ─────────────────────────────────

#[test]
fn spot_price_matches_the_companion() {
    let mut rng = Xorshift64::new(0x5907);
    for _ in 0..100_000 {
        let q = rng.next() >> rng.below(64);
        let vq = rng.next() >> rng.below(64);
        let b = rng.next() >> rng.below(64);
        let vb = rng.next() >> rng.below(64);
        assert_eq!(spot_price(q, vq, b, vb), real_spot_price(q, vq, b, vb), "{q} {vq} {b} {vb}");
    }
}

#[test]
fn step_toward_matches_the_companion() {
    let mut rng = Xorshift64::new(0x57E9);
    for _ in 0..100_000 {
        // The companion multiplies without a check, so stay where that cannot overflow.
        let reference = u128::from(rng.next()) << rng.below(50);
        let spot = u128::from(rng.next()) << rng.below(50);
        let reference = if rng.below(10) == 0 { 0 } else { reference };
        assert_eq!(
            step_toward(reference, spot, DIP.ref_step_bps),
            real_step_toward(reference, spot),
            "reference {reference} spot {spot}"
        );
    }
}

/// The body of the companion's private `catch_up` (steps.rs), on its real `step_toward` and constants.
fn real_catch_up(reference: u128, spot: u128, reference_at: i64, interval: i64, now: i64) -> u128 {
    let steps = (now.saturating_sub(reference_at) / interval.max(1)).clamp(1, MAX_REFERENCE_STEPS);
    let mut r = reference;
    for _ in 0..steps {
        let next = real_step_toward(r, spot);
        if next == r {
            break;
        }
        r = next;
    }
    r
}

#[test]
fn catch_up_matches_the_companion() {
    let mut rng = Xorshift64::new(0xCA7C);
    for _ in 0..100_000 {
        let reference = u128::from(rng.next()) << rng.below(50);
        let spot = u128::from(rng.next()) << rng.below(50);
        let at = rng.below(2_000_000) as i64;
        let now = at + rng.below(20_000_000) as i64 - 1_000;
        let interval = rng.below(600) as i64;
        assert_eq!(
            catch_up(reference, spot, at, interval, now, DIP.ref_step_bps, DIP.max_ref_steps),
            real_catch_up(reference, spot, at, interval, now),
            "reference {reference} spot {spot} at {at} now {now} interval {interval}"
        );
    }
}

// ─── choppy markets: the rule is a pullback rule, not a price-is-falling rule ─────────────────────

#[test]
fn a_noisy_climb_is_still_bought_on_its_pullbacks() {
    // +0.05% a minute with a +-3.5% wobble: the reference follows the highs, so a 3% pullback from
    // the recent high is a dip even while the coin climbs. This pins what the listing must say
    // ("buys pullbacks, about every five minutes in choppy markets"), not what it would like to say.
    let mut rng = Xorshift64::new(0xC40B);
    let mut state = Ref::default();
    let mut spot = 1_000_000_000_000u128;
    let (mut buys, pile) = (0u32, 50_000_000_000u64);
    for minute in 0..1_440i64 {
        let wobble = rng.below(7_001) as i128 - 3_500; // -3.5% .. +3.5% in basis points
        let next = spot as i128 * (10_000 + 5 + wobble) / 10_000;
        spot = next.max(1) as u128;
        let spent = simulate_step(&mut state, pile, 500_000_000_000, spot, None, minute * STEP);
        if spent > 0 {
            buys += 1;
        }
    }
    assert!(buys > 20, "a noisy climb should still be bought on pullbacks, got {buys}");
    assert!(buys <= 288, "never more than one buy per cooldown: {buys}");
}
