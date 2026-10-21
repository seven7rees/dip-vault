//! The numbers this script runs on. Every setting is a named constant here and nowhere else.
//!
//! Dip Vault: the pot (the companion fee plus the hook's 1% brake) is split every step. The
//! builder's share scales with the pool's real SOL (`BUILDER_SCALE`); the rest goes in three equal
//! parts to the dip buys (the coins bought are kept for liquidity, never burned), the weekly
//! holder payouts and the locked liquidity adds, which pair the coins held with SOL.

use anchor_lang::prelude::Pubkey;

/// All the settings in one place.
pub struct Params {
    /// Each dip buy is 1/`pace_steps` of the dip pile, times the dip rule's scale.
    pub pace_steps: u32,
    /// The coin must have this script's hook (a coin whose hook setup differs is refused).
    pub needs_hook: bool,
    /// The smallest buy, in lamports. A pile below this does not buy.
    pub min_buy: u64,
    /// The most a buy may be of the pool's real SOL, in basis points (100 is 1%); a launch with low
    /// fees gets less (`math::pool_share_bps`).
    pub max_buy_pool_bps: u16,
    /// A buy must deliver at least this share of the fee-free pool price, in basis points.
    pub min_out_bps: u16,
    /// Upper bound, in lamports, on what the companion pays the step's sender from the pot.
    /// The companion's own limit is 20_000.
    pub bounty_bound: u64,
    /// Whether the script buys only dips (`dip.rs`). Always true for Dip Vault.
    pub dip_only: bool,
}

/// The dip rule's settings.
pub struct Dip {
    /// A buy needs the price at least this far under the reference, in basis points (800 is 8%).
    /// The same drop is a buy of 1x the base size; the size grows in a straight line from there.
    pub trigger_bps: u16,
    /// The reference moves toward the price by at most this share of itself per move.
    pub ref_step_bps: u16,
    /// One step catches the reference up by at most this many moves.
    pub max_ref_steps: u32,
    /// The biggest multiple of the base size a deep dip buys (the pool and pile caps still apply).
    pub max_mult: u32,
    /// The least time between two buys, in seconds.
    pub cooldown_secs: i64,
    /// How often steps run, in seconds (keeper.json `stepSecs`): the reference moves once per this.
    pub step_secs: i64,
}

/// The dip rule's values: a buy at 8% under the reference, at most one every 5 minutes.
pub const DIP: Dip = Dip {
    trigger_bps: 800,
    ref_step_bps: 500,
    max_ref_steps: 64,
    max_mult: 4,
    cooldown_secs: 300,
    step_secs: 300,
};

/// The settings of this variant.
pub const PARAMS: Params = Params {
    pace_steps: 48,
    needs_hook: true,
    min_buy: 500_000,
    max_buy_pool_bps: 100,
    min_out_bps: 9_000,
    bounty_bound: 100_000,
    dip_only: true,
};

/// The wallet that receives the builder's share of new pot money.
pub const BUILDER_WALLET: Pubkey =
    Pubkey::from_str_const("AxoN4hpQJWKFkSimhqnA7dUsqf5VGojA5RhhZTiP4KFm");

// ─── The builder's share: 1% to 10% of new pot money, by the pool's real SOL ─────────────────

/// One step of the builder scale: from this many lamports of real SOL in the pool, this share of
/// new pot money, in basis points. Steps are in rising order; the first starts at 0.
pub struct BuilderStep {
    pub from_lamports: u64,
    pub bps: u16,
}

/// The builder scale: 1% under 5 SOL of real pool SOL, 2.5% from 5, 5% from 20, 7.5% from 50,
/// 10% from 100 SOL. (Proposed; the owner sets the final steps.)
pub const BUILDER_SCALE: [BuilderStep; 5] = [
    BuilderStep { from_lamports: 0, bps: 100 },
    BuilderStep { from_lamports: 5_000_000_000, bps: 250 },
    BuilderStep { from_lamports: 20_000_000_000, bps: 500 },
    BuilderStep { from_lamports: 50_000_000_000, bps: 750 },
    BuilderStep { from_lamports: 100_000_000_000, bps: 1_000 },
];

/// The most the builder scale ever answers: its last step.
pub const BUILDER_MAX_BPS: u16 = 1_000;

// ─── The three thirds of what is left after the builder's share ─────────────────────────────

/// The dip buyback's share of the rest, in basis points.
pub const DIP_THIRD_BPS: u16 = 3_333;
/// The weekly holder payouts' share of the rest, in basis points.
pub const HOLDER_THIRD_BPS: u16 = 3_333;
/// The locked liquidity's share of the rest: whatever the other two thirds leave (3,334 bps).
pub const LP_THIRD_BPS: u16 = 10_000 - DIP_THIRD_BPS - HOLDER_THIRD_BPS;

// ─── Holder payouts: weekly, weighted by amount and by hold time ─────────────────────────────

/// A payout round: a week. The keeper's epoch for the hook data's round field is the same number.
pub const ROUND_SECS: u32 = 604_800;
/// A holding earns nothing before its hold clock reaches this: 6 hours.
pub const MIN_HOLD_SECS: i64 = 21_600;
/// Hold-time weights, in basis points of the balance: 1x from 6 hours, 1.5x from 7 days, 2x from
/// 30 days. Tiers in rising order.
pub struct WeightTier {
    pub from_secs: i64,
    pub bps: u16,
}
pub const WEIGHT_TIERS: [WeightTier; 3] = [
    WeightTier { from_secs: 21_600, bps: 10_000 },
    WeightTier { from_secs: 604_800, bps: 15_000 },
    WeightTier { from_secs: 2_592_000, bps: 20_000 },
];
/// A holding that has not sold into the pool since it was funded weighs this much more, in basis
/// points of its weight: 10%. Set to 0 to turn the bonus off.
pub const NEVER_SOLD_BONUS_BPS: u16 = 1_000;
/// The most holdings one step pays or measures (the companion pays at most 10 a step).
pub const PAYOUT_MAX_ENTRIES: usize = 8;
/// A payout under this many lamports is not sent; the holding is measured (touched) instead
/// and its weight still counts: 0.0001 SOL.
pub const MIN_PAYOUT_LAMPORTS: u64 = 100_000;

// ─── Locked liquidity: a tranche every other step, dripped over about four hours ─────────────

/// Each liquidity add spends 1/`LP_PACE_STEPS` of the liquidity pile: at one add per
/// `LP_EVERY_SECS` (10 minutes), 24 adds drip a pile over about 4 hours.
pub const LP_PACE_STEPS: u32 = 24;
/// The least a liquidity add spends, in lamports: 0.002 SOL (the buy half and the pairing half).
pub const LP_MIN_ADD: u64 = 2_000_000;
/// The least time between two liquidity adds: 10 minutes (every other step).
pub const LP_EVERY_SECS: i64 = 600;
/// The share of a tranche that buys the coin; the rest pairs with the coins bought. A little under
/// half, because the buy pays the launch's fees and the pairing side does not.
pub const LP_BUY_SHARE_BPS: u16 = 4_850;
