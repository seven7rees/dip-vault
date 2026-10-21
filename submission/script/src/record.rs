//! A holding's record as the Dip Vault hook writes it: the hook data layout, byte for byte
//! (`hook_variants/dipvault/src/clock.rs`), and the weight a holding earns from it.
//!
//! # Hook data layout (64 bytes per holding, little-endian)
//! offset  0 : version     u8   1; 0 means no record
//! offset  1 : round       u32  the payout round this holding was last measured or paid in
//! offset  5 : stamp       i64  the hold clock: the weighted-average entry time (unix seconds)
//! offset 13 : amount      u64  the balance as of the last write
//! offset 21 : paid        u64  lamports paid to this holding, all time
//! offset 29 : entered_at  i64  when the holding last went from empty to funded
//! offset 37 : flags       u8   bit 0: sold into the pool since `entered_at`
//! offset 38 : measured    u64  the balance when the companion last recorded a payout or a touch
//! offset 46 : reserved    18 bytes, zero

use crate::params::{MIN_HOLD_SECS, NEVER_SOLD_BONUS_BPS, WEIGHT_TIERS};

/// Hook data per holding, in bytes.
pub const HOOK_DATA_LEN: usize = 64;
/// Layout version of a record; 0 is no record.
pub const RECORD_VERSION: u8 = 1;
/// `flags` bit 0: sold into the pool since the holding was funded.
pub const FLAG_SOLD: u8 = 1;
/// Basis points in a whole.
const BPS: u128 = 10_000;

/// Where each field sits.
pub mod record_offsets {
    pub const VERSION: usize = 0;
    pub const ROUND: usize = 1;
    pub const STAMP: usize = 5;
    pub const AMOUNT: usize = 13;
    pub const PAID: usize = 21;
    pub const ENTERED_AT: usize = 29;
    pub const FLAGS: usize = 37;
    pub const MEASURED: usize = 38;
}

/// A holding's record.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Record {
    pub version: u8,
    pub round: u32,
    pub stamp: i64,
    pub amount: u64,
    pub paid: u64,
    pub entered_at: i64,
    pub flags: u8,
    pub measured: u64,
}

fn u64_at(data: &[u8], at: usize) -> u64 {
    let mut b = [0u8; 8];
    b.copy_from_slice(&data[at..at + 8]);
    u64::from_le_bytes(b)
}

impl Record {
    /// Reads hook data. All zeros is no record.
    pub fn decode(data: &[u8; HOOK_DATA_LEN]) -> Self {
        use record_offsets as o;
        let mut round = [0u8; 4];
        round.copy_from_slice(&data[o::ROUND..o::ROUND + 4]);
        Self {
            version: data[o::VERSION],
            round: u32::from_le_bytes(round),
            stamp: u64_at(data, o::STAMP) as i64,
            amount: u64_at(data, o::AMOUNT),
            paid: u64_at(data, o::PAID),
            entered_at: u64_at(data, o::ENTERED_AT) as i64,
            flags: data[o::FLAGS],
            measured: u64_at(data, o::MEASURED),
        }
    }

    /// Whether the holding has a record.
    pub fn is_set(&self) -> bool {
        self.version == RECORD_VERSION
    }

    /// Whether the holding sold into the pool since it was funded.
    pub fn sold(&self) -> bool {
        self.flags & FLAG_SOLD != 0
    }
}

/// The hold-time multiplier of a holding whose clock reads `held` seconds, in basis points of its
/// balance: 0 before `MIN_HOLD_SECS`, then the highest tier reached. Never panics.
pub fn tier_bps(held: i64) -> u16 {
    if held < MIN_HOLD_SECS {
        return 0;
    }
    let mut bps = 0;
    for t in WEIGHT_TIERS.iter() {
        if held >= t.from_secs {
            bps = t.bps;
        }
    }
    bps
}

/// A holding's payout weight as of `at` (a round's first second): `balance` times its hold-time
/// multiplier at that time, plus the never-sold bonus. 0 for a holding under the least hold then,
/// or with no record. Measuring a round and paying it the round after use the same `at` and the
/// balance capped at the one measured, so the shares paid never add up past the budget.
pub fn weight(r: &Record, balance: u64, at: i64) -> u128 {
    if !r.is_set() {
        return 0;
    }
    let held = at.saturating_sub(r.stamp);
    let tier = u128::from(tier_bps(held));
    if tier == 0 {
        return 0;
    }
    let bonus = if r.sold() { 0 } else { u128::from(NEVER_SOLD_BONUS_BPS) };
    u128::from(balance).saturating_mul(tier) / BPS * (BPS + bonus) / BPS
}
