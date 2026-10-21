//! The weekly holder payouts: rounds, what one holding is due, and the touch rule.
//!
//! A round is a week (`ROUND_SECS`), numbered `floor(now / ROUND_SECS)` like the keeper's epoch.
//! Every holding the keeper brings is measured once a round: its weight (`record::weight`) joins
//! the round's total `w_cur`, and the hook records the round in its hook data so it is not brought
//! again until the next one. It is paid from the round's budget (the holder pile at the round's
//! first step) by its share of the total measured the round before: `budget * w / w_prev`. The
//! first round measures only; from the second on, each round pays what the previous week's
//! holders earned. Weight measured in a round beyond the round before's total is paid up to what
//! the budget has left, first come first served, and a budget not fully paid stays in the pile.

use crate::math::share;
use crate::params::MIN_PAYOUT_LAMPORTS;

/// The round `now` falls in: `floor(now / round_secs)`, 0 before 1970; never panics.
pub fn round_of(now: i64, round_secs: u32) -> u32 {
    if round_secs == 0 || now <= 0 {
        return 0;
    }
    u32::try_from((now as u64) / u64::from(round_secs)).unwrap_or(u32::MAX)
}

/// The round's counters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Round {
    pub round: u32,
    /// The total weight measured the round before (0: this round measures only).
    pub w_prev: u128,
    /// The weight measured so far this round.
    pub w_cur: u128,
    /// The round's budget: the holder pile at its first step.
    pub budget: u64,
    /// Paid so far this round.
    pub paid: u64,
    /// Holdings measured this round.
    pub measured: u32,
}

/// The counters rolled to round `now_round` when it is later than the current one: the weight
/// measured becomes the total to pay by (0 after a skipped round: nobody was measured), the
/// budget is `holder_pile`, the rest clears. Unchanged within the round.
pub fn roll(r: Round, now_round: u32, holder_pile: u64) -> Round {
    if now_round <= r.round {
        return r;
    }
    Round {
        round: now_round,
        w_prev: if now_round == r.round.saturating_add(1) {
            r.w_cur
        } else {
            0
        },
        w_cur: 0,
        budget: holder_pile,
        paid: 0,
        measured: 0,
    }
}

/// What a holding of weight `w` is paid this round: its share of the budget by the previous
/// round's total, never over what the budget has left, the holder pile, the step's remaining
/// budget or the per-holding cap; under `MIN_PAYOUT_LAMPORTS` it is a touch (0). 0 when the
/// round measures only (`w_prev` 0) or the weight is 0.
pub fn due(r: &Round, w: u128, holder_pile: u64, step_left: u64, holding_cap: u64) -> u64 {
    if w == 0 || r.w_prev == 0 {
        return 0;
    }
    let pay = share(r.budget, w, r.w_prev)
        .min(r.budget.saturating_sub(r.paid))
        .min(holder_pile)
        .min(step_left)
        .min(holding_cap);
    if pay < MIN_PAYOUT_LAMPORTS {
        0
    } else {
        pay
    }
}
