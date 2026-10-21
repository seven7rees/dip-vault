//! The numbers this script runs on. Every setting is a named constant here and nowhere else.

use anchor_lang::prelude::Pubkey;

/// All the settings in one place.
pub struct Params {
    /// The builder's share of new pot money, in basis points (2_000 is 20%).
    pub payout_bps: u16,
    /// Each step buys 1/`pace_steps` of the buy pile (480 for a step a minute is about 8 hours).
    pub pace_steps: u32,
    /// Whether the coin must have a custom hook. A coin whose hook setup does not match is refused.
    pub needs_hook: bool,
    /// The smallest buy, in lamports. A pile below this does not buy.
    pub min_buy: u64,
    /// The most a buy may be of the pool's real SOL, in basis points (100 is 1%); a launch with low
    /// fees gets less (`math::pool_share_bps`).
    pub max_buy_pool_bps: u16,
    /// The buy must deliver at least this share of the fee-free pool price, in basis points.
    pub min_out_bps: u16,
    /// Upper bound, in lamports, on what the companion pays the step's sender from the pot.
    /// The companion's own limit is 100_000.
    pub bounty_bound: u64,
    /// Whether the script buys only dips (see `dip.rs`); when false it buys at its pace every step.
    pub dip_only: bool,
}

/// The dip rule's settings, used only when `dip_only` is true.
pub struct Dip {
    /// A buy needs the price at least this far under the reference, in basis points (300 is 3%).
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

/// The dip rule's values.
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
    payout_bps: 1_000,
    pace_steps: 48,
    needs_hook: false,
    min_buy: 500_000,
    max_buy_pool_bps: 100,
    min_out_bps: 9_000,
    bounty_bound: 100_000,
    dip_only: true,
};

/// The wallet that receives the builder's share of new pot money.
pub const BUILDER_WALLET: Pubkey =
    Pubkey::from_str_const("AxoN4hpQJWKFkSimhqnA7dUsqf5VGojA5RhhZTiP4KFm");
