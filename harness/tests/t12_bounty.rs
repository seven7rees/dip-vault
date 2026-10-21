//! T12 — the step bounty: `math::step_bounty` against the companion's own `bounty_for`, and the pot
//! the script remembers after a step.
//!
//! The companion pays the step's sender out of the pot after the script ran (`pending_pot = pot -
//! paid - bounty`). If the script remembered the pot without that bounty, the next step would read
//! `pot - last_pot` as the new money less the bounty, and the builder would be short-changed every
//! step (all of it when inflow is under the bounty). The simulation below uses the companion's own
//! arithmetic, so it fails if the script's idea of the pot ever drifts from the companion's.

use bordrless_companion::constants::{PAYOUT_BOUNTY_FULL, SCRIPT_BOUNTY};
use bordrless_companion::instructions::bounty_for;
use workshop_script::math::{
    accrue, bps128, buy_pile, pot_after_step, step_bounty, COMPANION_PAYOUT_BOUNTY_FULL,
    COMPANION_SCRIPT_BOUNTY,
};

#[test]
fn constants_match_the_companion() {
    assert_eq!(COMPANION_SCRIPT_BOUNTY, SCRIPT_BOUNTY);
    assert_eq!(COMPANION_PAYOUT_BOUNTY_FULL, PAYOUT_BOUNTY_FULL);
}

#[test]
fn step_bounty_matches_the_companion() {
    for pot in [0u64, 1, 2_499, 2_500, 2_501, 19_999, 20_000, 20_001, 1_000_000, u64::MAX] {
        for entries in [None, Some(0u64), Some(1), Some(2), Some(7), Some(8), Some(9), Some(16), Some(1_000)] {
            let wanted = bounty_for(entries.map(|n| n as usize));
            let want = if pot >= wanted { wanted } else { 0 };
            assert_eq!(step_bounty(pot, entries), want, "pot {pot} entries {entries:?}");
        }
    }
}

struct Xorshift64(u64);
impl Xorshift64 {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
}

/// A day of steps, the companion's arithmetic on one side and the script's bookkeeping on the other.
fn day(inflow_of: impl Fn(u64, &mut Xorshift64) -> u64, pay_each_step: bool) -> (u64, u64) {
    let mut rng = Xorshift64(0xB0_0717);
    let (mut pot, mut last_pot, mut owed, mut unsplit) = (0u64, 0u64, 0u64, 0u64);
    let (mut ideal, mut paid_total) = (0u64, 0u64);
    for step in 0..1_440u64 {
        let inflow = inflow_of(step, &mut rng);
        pot += inflow;
        // The script: new money, then the builder's tally.
        let new_money = pot.saturating_sub(last_pot);
        ideal += bps128(inflow, 1_000);
        let (o, u) = accrue(owed, unsplit, new_money, true, 1_000);
        owed = o;
        unsplit = u;
        // It pays what it can afford when asked to (one entry), else answers nothing.
        let pay = if pay_each_step { owed.min(pot / 10) } else { 0 };
        let entries = if pay > 0 { Some(1u64) } else { None };
        // The buy pile the script would size a buy from must never dip into what the builder is owed.
        let pile = buy_pile(pot, owed, unsplit, 1_000);
        assert_eq!(pile, pot.saturating_sub(owed), "the pile must leave what is owed (it is 0 when the pot cannot cover it)");
        owed -= pay;
        paid_total += pay;
        let pot_after_script = pot;
        last_pot = pot_after_step(pot_after_script, pay);
        // The companion: pending_pot = pot - paid - bounty.
        let companion_bounty = {
            let wanted = bounty_for(entries.map(|n| n as usize));
            if pot_after_script >= wanted { wanted } else { 0 }
        };
        pot = pot_after_script - pay - companion_bounty;
        assert_eq!(pot, last_pot, "step {step}: the script's pot drifted from the companion's");
    }
    (ideal, paid_total + owed)
}

#[test]
fn builder_is_not_short_changed_by_the_bounty() {
    // Inflow of 20_000, the bounty itself, 100_000 and 1_000_000 lamports a step; no payouts,
    // then payouts every step. Within one lamport of rounding per step of the ideal 10%.
    for inflow in [20_000u64, 100_000, 1_000_000] {
        for pay in [false, true] {
            let (ideal, got) = day(|_, _| inflow, pay);
            assert!(got <= ideal, "inflow {inflow} pay {pay}: more than the ideal ({got} vs {ideal})");
            assert!(ideal - got <= 1_440, "inflow {inflow} pay {pay}: short by {} of {ideal}", ideal - got);
        }
    }
}

#[test]
fn random_inflows_stay_in_step() {
    let (ideal, got) = day(|_, rng| rng.next() % 3_000_000, true);
    assert!(got <= ideal && ideal - got <= 1_440, "short by {} of {ideal}", ideal - got);
}
