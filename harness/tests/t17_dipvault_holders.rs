//! T17 — Dip Vault script: the weekly holder payouts (DIP-VAULT.md item 5): weights, rounds,
//! what a holding is due, and that a wallet that sells before 6 hours earns nothing.

use dipvault_script::holders::{decide, due, earned, held_since_round_start, roll, round_of, round_start, Decision, Round};
use dipvault_script::params::{MIN_HOLD_SECS, MIN_PAYOUT_LAMPORTS, NEVER_SOLD_BONUS_BPS, ROUND_SECS};
use dipvault_script::record::{tier_bps, weight, Record, FLAG_SOLD};

const NOW: i64 = 1_800_000_000;
const WEEK: i64 = 604_800;
const DAY: i64 = 86_400;

fn rec(held: i64, amount: u64, sold: bool) -> Record {
    Record {
        version: 1,
        round: 0,
        stamp: NOW - held,
        amount,
        paid: 0,
        entered_at: NOW - held,
        flags: if sold { FLAG_SOLD } else { 0 },
        measured: amount,
    }
}

#[test]
fn the_weight_tiers() {
    assert_eq!(MIN_HOLD_SECS, 6 * 3_600);
    assert_eq!(tier_bps(0), 0);
    assert_eq!(tier_bps(6 * 3_600 - 1), 0, "nothing before 6 hours");
    assert_eq!(tier_bps(6 * 3_600), 10_000, "1x at 6 hours");
    assert_eq!(tier_bps(7 * DAY - 1), 10_000);
    assert_eq!(tier_bps(7 * DAY), 15_000, "1.5x at 7 days");
    assert_eq!(tier_bps(30 * DAY - 1), 15_000);
    assert_eq!(tier_bps(30 * DAY), 20_000, "2x at 30 days");
    assert_eq!(tier_bps(i64::MAX), 20_000);
    assert_eq!(tier_bps(i64::MIN), 0);
}

#[test]
fn a_holdings_weight_is_amount_times_tier_plus_the_never_sold_bonus() {
    assert_eq!(NEVER_SOLD_BONUS_BPS, 1_000);
    assert_eq!(weight(&rec(DAY, 1_000, true), 1_000, NOW), 1_000, "1x, sold once: no bonus");
    assert_eq!(weight(&rec(DAY, 1_000, false), 1_000, NOW), 1_100, "1x plus 10%");
    assert_eq!(weight(&rec(8 * DAY, 1_000, true), 1_000, NOW), 1_500);
    assert_eq!(weight(&rec(31 * DAY, 1_000, false), 1_000, NOW), 2_200);
    assert_eq!(weight(&rec(3_600, 1_000, false), 1_000, NOW), 0, "under 6 hours: nothing");
    assert_eq!(weight(&Record::default(), 1_000, NOW), 0, "no record: nothing");
    assert_eq!(weight(&rec(31 * DAY, u64::MAX, false), u64::MAX, NOW), u128::from(u64::MAX) * 2 * 11 / 10, "no overflow");
}

#[test]
fn rounds_are_weeks_since_1970_like_the_keepers_epoch() {
    assert_eq!(ROUND_SECS, 604_800);
    assert_eq!(round_of(NOW, ROUND_SECS), (NOW / WEEK) as u32);
    assert_eq!(round_of(0, ROUND_SECS), 0);
    assert_eq!(round_of(-1, ROUND_SECS), 0);
    assert_eq!(round_of(NOW, 0), 0, "never divides by zero");
    assert_eq!(round_of(i64::MAX, 1), u32::MAX, "never overflows");
}

#[test]
fn a_roll_carries_the_measured_weight_into_the_next_round_only() {
    let r = Round { round: 10, w_prev: 5, w_cur: 77, budget: 1, paid: 1, measured: 3 };
    assert_eq!(roll(r, 10, 999), r, "within the round: unchanged");
    assert_eq!(roll(r, 9, 999), r, "an earlier clock: unchanged");
    let next = roll(r, 11, 999);
    assert_eq!(next, Round { round: 11, w_prev: 77, w_cur: 0, budget: 999, paid: 0, measured: 0 });
    let skipped = roll(r, 13, 999);
    assert_eq!(skipped.w_prev, 0, "a skipped round: nobody was measured, this round measures only");
}

#[test]
fn what_a_holding_is_due() {
    let r = Round { round: 11, w_prev: 1_000, w_cur: 0, budget: 10_000_000, paid: 0, measured: 0 };
    assert_eq!(due(&r, 100, u64::MAX, u64::MAX, u64::MAX), 1_000_000, "a tenth of the weight: a tenth of the budget");
    assert_eq!(due(&r, 0, u64::MAX, u64::MAX, u64::MAX), 0);
    let first = Round { w_prev: 0, ..r };
    assert_eq!(due(&first, 100, u64::MAX, u64::MAX, u64::MAX), 0, "the first round measures only");
    assert_eq!(due(&r, 100, 300_000, u64::MAX, u64::MAX), 0, "the pile cannot cover it: skipped, paid later, never cut");
    assert_eq!(due(&r, 100, u64::MAX, 250_000, u64::MAX), 0, "the step's budget cannot: skipped");
    assert_eq!(due(&r, 100, u64::MAX, u64::MAX, 200_000), 0, "over the holding cap: skipped");
    let spent = Round { paid: 9_950_000, ..r };
    assert_eq!(due(&spent, 100, u64::MAX, u64::MAX, u64::MAX), 0, "under the least payout: a touch");
    assert_eq!(due(&spent, 500, u64::MAX, u64::MAX, u64::MAX), 0, "what the budget has left is under the least payout");
    let near = Round { paid: 9_000_000, ..r };
    assert_eq!(due(&near, 500, u64::MAX, u64::MAX, u64::MAX), 1_000_000, "what the budget has left");
    assert_eq!(due(&r, 10, u64::MAX, u64::MAX, u64::MAX), MIN_PAYOUT_LAMPORTS, "exactly the least payout is paid");
    assert_eq!(due(&r, 9, u64::MAX, u64::MAX, u64::MAX), 0, "a lamport under it is a touch");
    let huge = Round { w_prev: 1, budget: u64::MAX, ..r };
    assert_eq!(due(&huge, u128::MAX, u64::MAX, u64::MAX, u64::MAX), u64::MAX, "no overflow");
}

/// Two rounds over a set of wallets: the first measures, the second pays by the first's weights,
/// and the sum paid never exceeds the round's budget.
#[test]
fn a_week_measured_then_a_week_paid_within_the_budget() {
    let wallets: Vec<(u64, i64, bool)> = vec![
        (1_000_000, 2 * DAY, false),  // 1x + bonus
        (1_000_000, 2 * DAY, true),   // 1x
        (500_000, 10 * DAY, false),   // 1.5x + bonus
        (100_000, 40 * DAY, false),   // 2x + bonus
        (9_000_000, 3_600, false),    // under 6 hours: nothing
    ];
    let week1 = round_of(NOW, ROUND_SECS);
    let mut r = roll(Round::default(), week1, 5_000_000);
    let mut total_w = 0u128;
    for (amount, held, sold) in &wallets {
        let w = weight(&rec(*held, *amount, *sold), *amount, NOW);
        assert_eq!(due(&r, w, 5_000_000, u64::MAX, u64::MAX), 0, "week one measures only");
        r.w_cur += w;
        total_w += w;
    }
    assert!(total_w > 0);
    let w_under = weight(&rec(3_600, 9_000_000, false), 9_000_000, NOW);
    assert_eq!(w_under, 0, "the wallet under 6 hours has no weight");
    // Week two: the same wallets, a week older, paid by their week-one weights against the budget.
    let later = NOW + WEEK;
    let mut r = roll(r, week1 + 1, 5_000_000);
    assert_eq!(r.w_prev, total_w);
    let mut paid = 0u64;
    for (amount, held, sold) in &wallets {
        let w = weight(&rec(*held + WEEK, *amount, *sold), *amount, later);
        let pay = due(&r, w, 5_000_000 - paid, u64::MAX, u64::MAX);
        r.paid += pay;
        paid += pay;
    }
    assert!(paid <= 5_000_000, "never over the budget: {paid}");
    // Weights grow with the week held (some cross a tier), so the budget is spent first come
    // first served and the last wallets may get less: what is left stays in the pile.
    assert!(paid > 4_000_000, "most of the budget is paid: {paid}");
}

#[test]
fn a_wallet_that_sells_everything_before_six_hours_earns_nothing() {
    // Its record is cleared on the way out: no record, no weight, nothing to pay or measure.
    let cleared = Record::default();
    assert_eq!(weight(&cleared, 0, NOW), 0);
    let r = Round { round: 1, w_prev: 100, w_cur: 0, budget: 1_000_000, paid: 0, measured: 0 };
    assert_eq!(due(&r, weight(&cleared, 0, NOW), 1_000_000, u64::MAX, u64::MAX), 0);
}

#[test]
fn only_coins_held_since_before_the_round_began_count() {
    // Review finding (2026-10-21): coins moved to a fresh wallet mid-week restart their clock and
    // their round, so without this rule they could be paid again the same week.
    let r = round_of(NOW, ROUND_SECS);
    let start = round_start(r, ROUND_SECS);
    assert_eq!(start, i64::from(r) * WEEK);
    assert!(held_since_round_start(start, r, ROUND_SECS), "a clock at the round's first second counts");
    assert!(held_since_round_start(start - 1, r, ROUND_SECS));
    assert!(!held_since_round_start(start + 1, r, ROUND_SECS), "a clock started this week waits for next week");
    assert!(held_since_round_start(start + 1, r + 1, ROUND_SECS), "and counts next week");
    assert!(held_since_round_start(i64::MIN, u32::MAX, ROUND_SECS), "no overflow");
    assert!(!held_since_round_start(i64::MAX, 0, ROUND_SECS));
}

#[test]
fn a_step_out_of_budget_skips_a_holder_instead_of_marking_it_paid_nothing() {
    // Review finding (2026-10-21): the paying step marked holders beyond its budget as done with
    // 0 lamports, locking them out for the week.
    let r = Round { round: 11, w_prev: 1_000, w_cur: 0, budget: 10_000_000, paid: 0, measured: 0 };
    assert_eq!(earned(&r, 100), 1_000_000);
    assert_eq!(decide(&r, 100, u64::MAX, u64::MAX, u64::MAX), Decision::Pay(1_000_000));
    assert_eq!(decide(&r, 100, u64::MAX, 999_999, u64::MAX), Decision::Skip, "the step's budget is used up: wait");
    assert_eq!(decide(&r, 100, u64::MAX, u64::MAX, 999_999), Decision::Skip, "over the holding cap: wait");
    assert_eq!(decide(&r, 100, 999_999, u64::MAX, u64::MAX), Decision::Skip, "the pile cannot cover it: wait");
    assert_eq!(decide(&r, 0, u64::MAX, u64::MAX, u64::MAX), Decision::Touch, "no weight: measured, nothing due");
    assert_eq!(decide(&r, 5, u64::MAX, u64::MAX, u64::MAX), Decision::Touch, "under the least payout: a touch");
    let first = Round { w_prev: 0, ..r };
    assert_eq!(decide(&first, 100, u64::MAX, u64::MAX, u64::MAX), Decision::Touch, "a measuring round");
    let spent = Round { paid: 10_000_000, ..r };
    assert_eq!(decide(&spent, 100, u64::MAX, u64::MAX, u64::MAX), Decision::Touch, "the round's budget is gone: nothing earned");
    // The paying loop as the script runs it: eight holders of equal weight, a step budget of
    // 20% of a 10 SOL pot. Two are paid, six are skipped, none is marked.
    let mut round = Round { round: 11, w_prev: 8_000, w_cur: 0, budget: 3_333_000_000, paid: 0, measured: 0 };
    let mut step_left = 2_000_000_000u64;
    let (mut paid, mut skipped, mut touched) = (0, 0, 0);
    for _ in 0..8 {
        match decide(&round, 1_000, 3_333_000_000, step_left, u64::MAX) {
            Decision::Pay(x) => { paid += 1; step_left -= x; round.paid += x; round.measured += 1; }
            Decision::Skip => skipped += 1,
            Decision::Touch => touched += 1,
        }
    }
    assert_eq!((paid, skipped, touched), (4, 4, 0), "416,625,000 each: four fit in a 2 SOL step, four wait");
    assert_eq!(round.measured, 4, "only the paid ones are marked");
}

#[test]
fn shares_paid_the_round_after_never_add_up_past_the_budget() {
    // Measured at a round's first second; paid the next round with the same basis (hold time at
    // that first second, balance capped at the measured one), whatever grew in between.
    let start = round_start(round_of(NOW, ROUND_SECS), ROUND_SECS);
    let wallets = [(1_000_000u64, 2 * DAY), (500_000, 10 * DAY), (100_000, 40 * DAY), (2_000_000, 7 * 3_600)];
    let mut w_prev = 0u128;
    let mut recs = Vec::new();
    for (amount, held) in wallets {
        let r = Record { stamp: start - held, measured: amount, ..rec(held, amount, false) };
        w_prev += weight(&r, amount, start);
        recs.push(r);
    }
    // A week later every wallet is older (tiers rose) and two doubled their balance.
    let mut paid = 0u64;
    let budget = 10_000_000u64;
    for (i, r) in recs.iter().enumerate() {
        let balance_now = if i % 2 == 0 { r.measured * 2 } else { r.measured };
        let w = weight(r, balance_now.min(r.measured), start);
        paid += (budget as u128 * w / w_prev) as u64;
    }
    assert!(paid <= budget, "{paid} of {budget}");
    assert!(paid >= budget - 4, "and nearly all of it: {paid}");
}
