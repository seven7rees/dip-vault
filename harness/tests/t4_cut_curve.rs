//! T4 — Hook cut curve (SPEC F8).
//!
//! Verifies sell_cut_bps: table of expected values, monotonicity,
//! cap ≤ 780 (CUT_CAP_BPS), and no panic on edge cases.
//!
//! Formula: depth = qr + vq + amount_out
//!          if depth==0 or amount==0 → 0
//!          else min(amount_out * 7800 / depth, 780)

use my_hook::sell_cut_bps;

// ─── known-value table ────────────────────────────────────────────────────────

struct Case {
    amount_out:    u64,
    quote_reserve: u64,
    virtual_quote: u64,
    expected_bps:  u16,
    label:         &'static str,
}

fn table() -> Vec<Case> {
    // depth = qr + vq + amount; share = amount / depth
    // expected = floor(amount * 7800 / depth), capped at 780
    vec![
        // share ≈ 0.1%: depth=1_000_000, amount=1_000 → floor(7.8) = 7
        Case { amount_out: 1_000, quote_reserve: 999_000, virtual_quote: 0,       expected_bps: 7,   label: "0.1% share" },
        // share ≈ 1%: depth=1_000_000, amount=10_000 → 78
        Case { amount_out: 10_000, quote_reserve: 990_000, virtual_quote: 0,      expected_bps: 78,  label: "1% share" },
        // share ≈ 5%: depth=1_000_000, amount=50_000 → 390
        Case { amount_out: 50_000, quote_reserve: 950_000, virtual_quote: 0,      expected_bps: 390, label: "5% share" },
        // share ≈ 10%: depth=1_000_000, amount=100_000 → 780 (at cap)
        Case { amount_out: 100_000, quote_reserve: 900_000, virtual_quote: 0,     expected_bps: 780, label: "10% share (cap)" },
        // share ≈ 50%: capped at 780
        Case { amount_out: 500_000, quote_reserve: 500_000, virtual_quote: 0,     expected_bps: 780, label: "50% share (capped)" },
        // share ≈ 100%: amount_out == depth → capped at 780
        Case { amount_out: 1_000_000, quote_reserve: 0, virtual_quote: 0,         expected_bps: 780, label: "100% share (capped)" },
        // virtual_quote contributes to depth
        Case { amount_out: 1_000, quote_reserve: 0, virtual_quote: 999_000,       expected_bps: 7,   label: "virtual_quote depth" },
        // amount == 0 → 0
        Case { amount_out: 0, quote_reserve: 1_000_000, virtual_quote: 0,         expected_bps: 0,   label: "amount=0" },
        // all zero (depth == 0) → 0
        Case { amount_out: 0, quote_reserve: 0, virtual_quote: 0,                 expected_bps: 0,   label: "all-zero" },
    ]
}

#[test]
fn sell_cut_bps_table() {
    for c in table() {
        let got = sell_cut_bps(c.amount_out, c.quote_reserve, c.virtual_quote);
        assert_eq!(
            got, c.expected_bps,
            "[{}] sell_cut_bps({}, {}, {}) = {got}, want {}",
            c.label, c.amount_out, c.quote_reserve, c.virtual_quote, c.expected_bps
        );
    }
}

// ─── cap ≤ 780 ────────────────────────────────────────────────────────────────

#[test]
fn sell_cut_bps_never_exceeds_cap() {
    let cases: &[(u64, u64, u64)] = &[
        (u64::MAX, 0, 0),
        (u64::MAX, u64::MAX, u64::MAX),
        (1, 0, 0),
        (1_000_000, 0, 0),
        (999_999_999_999, 1, 1),
    ];
    for &(a, qr, vq) in cases {
        let cut = sell_cut_bps(a, qr, vq);
        assert!(
            cut <= 780,
            "sell_cut_bps({a}, {qr}, {vq}) = {cut}, exceeds CUT_CAP_BPS=780"
        );
    }
}

// ─── monotonicity ─────────────────────────────────────────────────────────────
//
// With fixed depth, a larger share of amount_out means a larger (or equal) cut.
// We test by fixing quote_reserve + virtual_quote and sweeping amount_out.

#[test]
fn sell_cut_bps_monotone_in_share() {
    let pool_depth: u64 = 10_000_000; // qr + vq (before amount)
    let steps = 200u64;

    let mut prev_cut = 0u16;
    for i in 1..=steps {
        // amount_out = i * pool_depth / steps  (increases each step)
        let amount_out = i * pool_depth / steps;
        let cut = sell_cut_bps(amount_out, pool_depth, 0);
        assert!(
            cut >= prev_cut,
            "monotone violated at step {i}: cut={cut} < prev={prev_cut} (amount_out={amount_out})"
        );
        prev_cut = cut;
    }
}

// ─── no panic on extreme inputs ───────────────────────────────────────────────

#[test]
fn sell_cut_bps_no_panic_extremes() {
    let _ = sell_cut_bps(u64::MAX, 0, 0);
    let _ = sell_cut_bps(u64::MAX, u64::MAX, 0);
    let _ = sell_cut_bps(u64::MAX, 0, u64::MAX);
    // depth overflow scenario (qr + vq + amount could overflow u128 if not careful)
    // max possible depth in u128 is u64::MAX * 3 — still fits in u128 (3 * 2^64 < 2^128)
    let _ = sell_cut_bps(u64::MAX, u64::MAX, u64::MAX);
    let _ = sell_cut_bps(0, u64::MAX, u64::MAX);
    let _ = sell_cut_bps(1, u64::MAX, u64::MAX);
}

// ─── CUT_SLOPE cross-check ────────────────────────────────────────────────────

#[test]
fn sell_cut_bps_slope_crosscheck() {
    // Exact break-even: amount/depth == CUT_CAP_BPS/CUT_SLOPE_BPS == 780/7800 == 1/10 == 10%
    // At exactly 10%, floor(amount*7800/depth) == 780 == cap.
    // Below 10%, floor < 780.
    let depth_base: u64 = 10_000_000;
    // 10% share: amount = 1_000_000, pool = 9_000_000
    let cut_at_10pct = sell_cut_bps(1_000_000, 9_000_000, 0);
    assert_eq!(cut_at_10pct, 780, "exactly 10% should hit cap");

    // 9.99%: amount = 999_000, pool = 9_001_000  (depth=10_000_000, amount=999_000)
    // floor(999_000 * 7800 / 10_000_000) = floor(779.22) = 779
    let cut_just_below = sell_cut_bps(999_000, 9_001_000, 0);
    assert_eq!(cut_just_below, 779, "9.99% should be 779 bps");

    // Verify monotone at boundary
    assert!(cut_just_below < cut_at_10pct);

    let _ = depth_base; // used in reasoning above
}
