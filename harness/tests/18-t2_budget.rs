//! T2 — step_budget parity: math.rs vs payout.rs (SPEC F6).
//!
//! Runs ≥100,000 random cases using a deterministic XorShift64 RNG.
//! Both functions must return identical results on every input.

use bordrless_companion::instructions::{step_budget as companion_step_budget, window_base as companion_window_base};
use workshop_script::math::{step_budget as script_step_budget, window_base as script_window_base};

struct Xorshift64(u64);

impl Xorshift64 {
    fn new(seed: u64) -> Self {
        Self(seed | 1) // ensure non-zero
    }
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn next_u16_range(&mut self, lo: u16, hi: u16) -> u16 {
        // lo..=hi
        let range = (hi - lo + 1) as u64;
        lo + (self.next() % range) as u16
    }
}

const ITERATIONS: u32 = 100_000;

#[test]
fn step_budget_parity_100k() {
    let mut rng = Xorshift64::new(0x12345678_ABCDEF01);
    let mut divergences = 0u32;

    for i in 0..ITERATIONS {
        let pot_before  = rng.next();
        let pot_after   = rng.next();
        // bounty must be ≤ pot_after (per real usage); clamp it
        let raw_bounty  = rng.next();
        let bounty      = raw_bounty.min(pot_after);
        let w_pot       = rng.next();
        let w_paid      = rng.next();
        let step_bps    = rng.next_u16_range(1, 10_000);
        let window_bps  = rng.next_u16_range(1, 10_000);

        let script    = script_step_budget(pot_before, pot_after, bounty, w_pot, w_paid, step_bps, window_bps);
        let companion = companion_step_budget(pot_before, pot_after, bounty, w_pot, w_paid, step_bps, window_bps);

        if script != companion {
            divergences += 1;
            if divergences <= 5 {
                eprintln!(
                    "iter {i}: DIVERGE script={script} companion={companion}  \
                     pot_before={pot_before} pot_after={pot_after} bounty={bounty} \
                     w_pot={w_pot} w_paid={w_paid} step_bps={step_bps} window_bps={window_bps}"
                );
            }
        }
    }

    assert_eq!(
        divergences, 0,
        "step_budget diverged on {divergences}/{ITERATIONS} cases"
    );
}

#[test]
fn window_base_parity_100k() {
    let mut rng = Xorshift64::new(0xABCDEF01_12345678);
    let mut divergences = 0u32;

    for i in 0..ITERATIONS {
        let pot_after = rng.next();
        let w_pot     = rng.next();
        let w_paid    = rng.next();

        let script    = script_window_base(pot_after, w_paid, w_pot);
        let companion = companion_window_base(pot_after, w_pot, w_paid);

        if script != companion {
            divergences += 1;
            if divergences <= 5 {
                eprintln!(
                    "iter {i}: DIVERGE script={script} companion={companion}  \
                     pot_after={pot_after} w_pot={w_pot} w_paid={w_paid}"
                );
            }
        }
    }

    assert_eq!(
        divergences, 0,
        "window_base diverged on {divergences}/{ITERATIONS} cases"
    );
}

#[test]
fn step_budget_edge_cases() {
    // All zeros → 0
    assert_eq!(script_step_budget(0, 0, 0, 0, 0, 100, 500), 0);
    assert_eq!(companion_step_budget(0, 0, 0, 0, 0, 100, 500), 0);

    // bounty == pot_after → pot_after - bounty == 0 → leg_c = 0
    let result = script_step_budget(1_000_000, 500_000, 500_000, 0, 0, 10_000, 10_000);
    assert_eq!(result, 0, "leg_c should be 0 when bounty == pot_after");

    // leg_a = bps128(pot_before, step_bps): with step_bps=10_000 and no window/bounty cap,
    // result = pot_before (leg_a == pot_before when step_bps=10_000 and all other legs ≥ it)
    let pot_before = 1_000_000_u64;
    let pot_after  = 1_000_000_u64; // no inflow (same as pot_before)
    let bounty     = 0_u64;
    // With w_pot=0, w_paid=0, step_bps=10_000, window_bps=10_000:
    //   a = bps128(1M, 10_000) = 1M
    //   wb = window_base(1M, 0, 0) = 1M
    //   b = bps128(1M, 10_000) - 0 = 1M
    //   c = 1M - 0 = 1M
    //   result = 1M
    let result = script_step_budget(pot_before, pot_after, bounty, 0, 0, 10_000, 10_000);
    assert_eq!(result, pot_before, "step_bps=10_000 → result=pot_before when all legs equal");

    // Saturation: u64::MAX inputs must not panic
    let _ = script_step_budget(u64::MAX, u64::MAX, 0, u64::MAX, u64::MAX, 10_000, 10_000);
    let _ = companion_step_budget(u64::MAX, u64::MAX, 0, u64::MAX, u64::MAX, 10_000, 10_000);
}

#[test]
fn step_budget_parity_on_saturating_inputs() {
    // Stress the overflow guard: very large values.
    let mut rng = Xorshift64::new(0xCAFEBABE_DEADBEEF);
    for _ in 0..10_000 {
        let pot_after  = rng.next() | (1 << 63); // top half of u64 range
        let pot_before = pot_after.saturating_add(rng.next() >> 1);
        let bounty     = (rng.next() >> 4).min(pot_after);
        let w_pot      = rng.next() | (1 << 62);
        let w_paid     = rng.next() | (1 << 62);
        let step_bps   = 10_000u16;
        let window_bps = 10_000u16;

        let s = script_step_budget(pot_before, pot_after, bounty, w_pot, w_paid, step_bps, window_bps);
        let c = companion_step_budget(pot_before, pot_after, bounty, w_pot, w_paid, step_bps, window_bps);
        assert_eq!(s, c, "large-value divergence");
    }
}
