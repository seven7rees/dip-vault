//! T5 — buy_amount property tests (SPEC step 4).
//!
//! Properties from the implementation:
//!   P1: result ≤ pile  (never spend more than available)
//!   P2: result ≤ pool_cap = bps(quote_reserve, max_buy_pool_bps)
//!   P3: result ≥ min_buy  (if Some)
//!   P4: pile < min_buy → None
//!   P5: pool_cap < min_buy → None (even when pile is large)
//!   P6: pace_steps == 0 treated as 1 (no divide-by-zero)

use workshop_script::math::buy_amount;

// MOON params from params.rs
const MIN_BUY:          u64 = 2_000_000;
const MAX_BUY_POOL_BPS: u16 = 100; // 1%
const PACE_STEPS:       u32 = 480;

fn pool_cap(qr: u64) -> u64 {
    // bps128: floor(qr * MAX_BUY_POOL_BPS / 10_000)
    (u128::from(qr) * u128::from(MAX_BUY_POOL_BPS) / 10_000) as u64
}

// ─── P4: pile < min_buy → None ───────────────────────────────────────────────

#[test]
fn buy_amount_none_when_pile_below_min_buy() {
    for pile in [0u64, 1, MIN_BUY - 1] {
        let result = buy_amount(pile, PACE_STEPS, MIN_BUY, MAX_BUY_POOL_BPS, 1_000_000_000);
        assert_eq!(result, None, "pile={pile} should give None");
    }
}

// ─── P5: pool_cap < min_buy → None ───────────────────────────────────────────

#[test]
fn buy_amount_none_when_pool_cap_below_min_buy() {
    // pool_cap = floor(qr * 1%) < min_buy when qr < min_buy * 100
    let small_qr = (MIN_BUY * 99) as u64; // pool_cap = floor(small_qr / 100) < MIN_BUY
    let cap = pool_cap(small_qr);
    assert!(cap < MIN_BUY, "test setup: cap={cap} must be < min_buy={MIN_BUY}");

    let pile = MIN_BUY * 1_000; // pile is huge, but pool cap gates it
    let result = buy_amount(pile, PACE_STEPS, MIN_BUY, MAX_BUY_POOL_BPS, small_qr);
    assert_eq!(result, None, "small pool cap should give None");
}

// ─── P1+P3: when Some, min_buy ≤ result ≤ pile ───────────────────────────────

#[test]
fn buy_amount_bounds_when_some() {
    let pile = 1_000_000_000_u64; // 1 SOL, plenty
    let qr   = 100_000_000_000_u64; // 100 SOL in pool

    let result = buy_amount(pile, PACE_STEPS, MIN_BUY, MAX_BUY_POOL_BPS, qr);
    let amount = result.expect("should return Some with generous inputs");

    assert!(amount >= MIN_BUY, "amount={amount} < min_buy={MIN_BUY}");
    assert!(amount <= pile,    "amount={amount} > pile={pile}");
}

// ─── P2: result ≤ pool_cap ───────────────────────────────────────────────────

#[test]
fn buy_amount_never_exceeds_pool_cap() {
    let pile = u64::MAX;
    let qr   = 1_000_000_000_u64;
    let cap  = pool_cap(qr);

    let result = buy_amount(pile, PACE_STEPS, MIN_BUY, MAX_BUY_POOL_BPS, qr);
    if let Some(amount) = result {
        assert!(
            amount <= cap,
            "amount={amount} > pool_cap={cap}"
        );
    }
}

// ─── P6: pace_steps == 0 (treated as 1) ──────────────────────────────────────

#[test]
fn buy_amount_pace_steps_zero_no_panic() {
    let result = buy_amount(MIN_BUY * 10, 0, MIN_BUY, MAX_BUY_POOL_BPS, 1_000_000_000);
    // pile / max(0,1) = pile — so raw = pile; capped by pool_cap and pile.
    // Should not panic.
    let _ = result;
}

// ─── Exact output check ───────────────────────────────────────────────────────

#[test]
fn buy_amount_exact_simple_case() {
    // pile = min_buy * pace_steps exactly → raw = min_buy → amount = min_buy (floor of pace)
    let pile = MIN_BUY * u64::from(PACE_STEPS);
    let huge_qr = pile * 1_000; // pool cap >> pile
    let result = buy_amount(pile, PACE_STEPS, MIN_BUY, MAX_BUY_POOL_BPS, huge_qr);
    let amount = result.expect("should be Some");
    assert_eq!(amount, MIN_BUY, "pile/PACE_STEPS should give min_buy exactly");
}

#[test]
fn buy_amount_pile_larger_than_pace() {
    // pile = 10 * min_buy * pace_steps → raw = 10 * min_buy → capped by pool_cap
    let pile = MIN_BUY * u64::from(PACE_STEPS) * 10;
    let qr   = 1_000_000_000_u64; // pool_cap = 10_000_000
    let cap  = pool_cap(qr);
    let result = buy_amount(pile, PACE_STEPS, MIN_BUY, MAX_BUY_POOL_BPS, qr);
    if let Some(amount) = result {
        assert!(amount >= MIN_BUY);
        assert!(amount <= cap);
        assert!(amount <= pile);
    }
}

// ─── Property-based sweep ─────────────────────────────────────────────────────

struct Xorshift64(u64);
impl Xorshift64 {
    fn new(s: u64) -> Self { Self(s | 1) }
    fn next(&mut self) -> u64 {
        let mut x = self.0; x ^= x << 13; x ^= x >> 7; x ^= x << 17;
        self.0 = x; x
    }
}

#[test]
fn buy_amount_property_sweep_10k() {
    let mut rng = Xorshift64::new(0xDEAD_C0DE_1234_5678);
    for _ in 0..10_000 {
        let pile = rng.next();
        let qr   = rng.next();
        let min_buy_r = (rng.next() % 10_000_000).max(1);
        let pool_bps  = ((rng.next() % 1000) as u16).max(1);
        let pace      = ((rng.next() % 1000) as u32).max(1);

        let result = buy_amount(pile, pace, min_buy_r, pool_bps, qr);
        if let Some(amount) = result {
            assert!(amount >= min_buy_r, "P3 violated: amount < min_buy");
            assert!(amount <= pile,      "P1 violated: amount > pile");
            let cap = (u128::from(qr) * u128::from(pool_bps) / 10_000) as u64;
            assert!(amount <= cap,       "P2 violated: amount > pool_cap");
        }
        // If None: either pile < min_buy or pool_cap < min_buy (consistent with impl)
        if result.is_none() {
            let cap = (u128::from(qr) * u128::from(pool_bps) / 10_000) as u64;
            let expected_none = pile < min_buy_r || cap < min_buy_r;
            assert!(expected_none, "unexpected None with pile={pile} min_buy={min_buy_r} cap={cap}");
        }
    }
}
