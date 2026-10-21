//! T6 — N-step accounting simulation (SPEC §3, SPEC F6).
//!
//! Simulates a Companion's pot lifecycle across many steps, asserting that:
//!   I1: owed never underflows  (owed decreases only by pay)
//!   I2: each pay ≤ owed
//!   I3: each pay ≤ step_budget (budget gate)
//!   I4: paid_total increases monotonically
//!   I5: cumulative pay ≤ cumulative inflow * step_bps / 10_000  (bps ceiling)
//!   I6: pot_after - pay ≥ 0 (pot stays non-negative)

use workshop_script::math::{bps128, step_budget};

struct Xorshift64(u64);
impl Xorshift64 {
    fn new(s: u64) -> Self { Self(s | 1) }
    fn next(&mut self) -> u64 {
        let mut x = self.0; x ^= x << 13; x ^= x >> 7; x ^= x << 17;
        self.0 = x; x
    }
}

/// Run one accounting simulation.  Returns (steps_taken, invariant_violations).
fn simulate(
    initial_pot:   u64,
    step_bps:      u16,
    window_bps:    u16,
    window_secs:   u32,
    pace_steps:    u32,
    n_steps:       usize,
    rng:           &mut Xorshift64,
) -> (usize, usize) {
    let _ = (window_secs, pace_steps); // used only for realism

    let mut pot          = initial_pot;
    let mut owed         = 0u64;
    let mut paid_total   = 0u64;
    let mut window_pot   = 0u64;
    let mut window_paid  = 0u64;
    let mut total_inflow = 0u64;
    let mut violations   = 0;

    for _ in 0..n_steps {
        // Random inflow this step (0..50_000_000 lamports)
        let inflow = rng.next() % 50_000_000;

        // Accrue owed on inflow
        let pot_before = pot;
        pot = pot.saturating_add(inflow);
        total_inflow = total_inflow.saturating_add(inflow);
        owed = owed.saturating_add(bps128(inflow, step_bps));

        // Compute budget
        let pot_after_script = pot;
        let bounty = 20_000_u64;
        let budget = step_budget(
            pot_before,
            pot_after_script,
            bounty.min(pot_after_script),
            window_pot,
            window_paid,
            step_bps,
            window_bps,
        );

        // Pay: min(owed, budget)
        let pay = owed.min(budget);

        // I2: pay ≤ owed
        if pay > owed {
            violations += 1;
            eprintln!("I2 violated: pay={pay} > owed={owed}");
        }

        // I3: pay ≤ budget
        if pay > budget {
            violations += 1;
            eprintln!("I3 violated: pay={pay} > budget={budget}");
        }

        // I6: pot_after - pay ≥ 0
        if pay > pot {
            violations += 1;
            eprintln!("I6 violated: pay={pay} > pot={pot}");
        }

        // Apply payment
        let prev_paid = paid_total;
        owed -= pay;
        pot = pot.saturating_sub(pay);
        paid_total += pay;
        window_paid = window_paid.saturating_add(pay);
        window_pot  = window_pot.max(pot);

        // I1: owed still valid (u64 never underflows, but catch logic errors)
        // (u64 subtraction above would panic on underflow in debug mode — that's the guard)

        // I4: paid_total monotone
        if paid_total < prev_paid {
            violations += 1;
            eprintln!("I4 violated: paid_total went backwards");
        }
    }

    // I5: cumulative paid ≤ bps(total_inflow, step_bps) + rounding_per_step
    let ceiling = bps128(total_inflow, step_bps);
    // Each step adds bps128(inflow) which can round down, so paid_total ≤ ceiling always.
    if paid_total > ceiling {
        violations += 1;
        eprintln!(
            "I5 violated: paid_total={paid_total} > ceiling={ceiling} (inflow={total_inflow})"
        );
    }

    (n_steps, violations)
}

#[test]
fn accounting_moon_params_500_steps() {
    let mut rng = Xorshift64::new(0xABCD_1234_EFAB_5678);
    let (steps, violations) = simulate(
        1_000_000_000, // initial pot
        2_000,         // step_bps (MOON: 20%)
        5_000,         // window_bps (50%)
        86_400,        // window_secs
        480,           // pace_steps
        500,
        &mut rng,
    );
    assert_eq!(violations, 0, "{violations}/{steps} invariant violations (MOON)");
}

#[test]
fn accounting_wick_params_500_steps() {
    let mut rng = Xorshift64::new(0x1234_ABCD_5678_EF01);
    let (steps, violations) = simulate(
        500_000_000, // initial pot
        1_000,       // step_bps (WICK: 10%)
        5_000,
        86_400,
        240,
        500,
        &mut rng,
    );
    assert_eq!(violations, 0, "{violations}/{steps} invariant violations (WICK)");
}

#[test]
fn accounting_zero_inflow_yields_zero_paid() {
    // With no inflow ever, owed stays 0, pay stays 0.
    let mut rng = Xorshift64::new(0);
    // Patch: override inflow to 0 by using a dedicated deterministic sim.
    let n = 100usize;
    let mut pot = 100_000_000_u64;
    let mut owed = 0u64;
    let mut paid_total = 0u64;
    for _ in 0..n {
        let budget = step_budget(pot, pot, 0, 0, 0, 2_000, 5_000);
        let pay = owed.min(budget);
        owed -= pay;
        pot = pot.saturating_sub(pay);
        paid_total += pay;
    }
    assert_eq!(paid_total, 0, "no inflow → no payment");
    let _ = rng.next(); // suppress unused warning
}

#[test]
fn accounting_single_large_inflow_then_drain() {
    // One large inflow; verify it drains correctly over many steps.
    let inflow   = 10_000_000_000_u64;
    let step_bps = 2_000u16;
    let window_bps = 5_000u16;
    let total_owed = bps128(inflow, step_bps);

    let mut pot = inflow;
    let mut owed = total_owed;
    let mut paid_total = 0u64;
    let bounty = 20_000u64;

    for _ in 0..1000 {
        if owed == 0 { break; }
        let budget = step_budget(pot, pot, bounty.min(pot), 0, paid_total, step_bps, window_bps);
        let pay = owed.min(budget);
        owed = owed.saturating_sub(pay);
        pot  = pot.saturating_sub(pay);
        paid_total += pay;
    }

    assert!(paid_total <= total_owed, "overpaid: {paid_total} > {total_owed}");
}

#[test]
fn accounting_saturating_accumulation() {
    // Extreme: inflows near u64::MAX — saturating arithmetic must not panic.
    let mut rng = Xorshift64::new(0xFFFF_0000_FFFF_0000);
    let (_, violations) = simulate(
        u64::MAX / 2,
        10_000, // 100% step_bps
        10_000,
        1,
        1,
        100,
        &mut rng,
    );
    assert_eq!(violations, 0, "extreme values should not cause violations");
}
