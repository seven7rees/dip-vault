//! The pure arithmetic of the Dip Vault hook: each holding's record (its 64 bytes of hook data),
//! the hold clock, the sell brake and the sell relay. Nothing here reads an account, and nothing
//! here can panic on any input: every sum is u128 or saturating.
//!
//! # Hook data layout (64 bytes per holding, little-endian; the companion script reads the same)
//! offset  0 : version     u8   1; 0 means no record (never written, or cleared when emptied)
//! offset  1 : round       u32  the weekly payout round this holding was last measured or paid in
//!                              (`record_payout`'s tag); 0 before the first. The keeper brings a
//!                              holding whose round is not the current epoch.
//! offset  5 : stamp       i64  the hold clock: the weighted-average entry time (unix seconds)
//! offset 13 : amount      u64  the balance as of the last write
//! offset 21 : paid        u64  lamports paid to this holding by the companion, all time
//! offset 29 : entered_at  i64  when the holding last went from empty to funded
//! offset 37 : flags       u8   bit 0: sold into the pool since `entered_at`
//! offset 38 : measured    u64  the balance when the companion last recorded a payout (a touch or
//!                              a payment); the companion script pays next round by at most this
//! offset 46 : reserved    18 bytes, zero

use bordrless_hook::HOOK_DATA_LEN;

/// Layout version of a holding's [`Record`]; 0 is no record.
pub const RECORD_VERSION: u8 = 1;
/// Basis points in a whole.
pub const BPS: u64 = 10_000;
/// The sell brake lasts this long after the hold clock: 10 minutes.
pub const BRAKE_SECS: i64 = 600;
/// The brake's SOL cut on a sell inside the window, in basis points of the SOL out: 80. The
/// launchpad adds a quarter on top, so the seller pays about 1% (`hook.json` caps sells at 80).
pub const BRAKE_BPS: u16 = 80;
/// The SOL cut after the window, and on every buy: none.
pub const FREE_BPS: u16 = 0;
/// `Record.flags` bit 0: the holding sold into the pool since it was last funded.
pub const FLAG_SOLD: u8 = 1;

/// Where each field sits in a holding's 64 bytes of hook data.
pub mod record_offsets {
    pub const VERSION: usize = 0;
    pub const ROUND: usize = 1;
    pub const STAMP: usize = 5;
    pub const AMOUNT: usize = 13;
    pub const PAID: usize = 21;
    pub const ENTERED_AT: usize = 29;
    pub const FLAGS: usize = 37;
    pub const MEASURED: usize = 38;
    pub const RESERVED: usize = 46;
}

/// A holding's record, as the hook data lays it out.
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
    /// Reads hook data. All zeros (never written, or cleared) is no record.
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

    /// The hook data to write (the reserved bytes zero).
    pub fn encode(&self) -> [u8; HOOK_DATA_LEN] {
        use record_offsets as o;
        let mut out = [0u8; HOOK_DATA_LEN];
        out[o::VERSION] = self.version;
        out[o::ROUND..o::ROUND + 4].copy_from_slice(&self.round.to_le_bytes());
        out[o::STAMP..o::STAMP + 8].copy_from_slice(&self.stamp.to_le_bytes());
        out[o::AMOUNT..o::AMOUNT + 8].copy_from_slice(&self.amount.to_le_bytes());
        out[o::PAID..o::PAID + 8].copy_from_slice(&self.paid.to_le_bytes());
        out[o::ENTERED_AT..o::ENTERED_AT + 8].copy_from_slice(&self.entered_at.to_le_bytes());
        out[o::FLAGS] = self.flags;
        out[o::MEASURED..o::MEASURED + 8].copy_from_slice(&self.measured.to_le_bytes());
        out
    }

    /// Whether the holding has a record (version 1).
    pub fn is_set(&self) -> bool {
        self.version == RECORD_VERSION
    }

    /// The hold clock, when the record is set.
    pub fn stamp_opt(&self) -> Option<i64> {
        self.is_set().then_some(self.stamp)
    }

    /// Whether the holding sold into the pool since it was funded.
    pub fn sold(&self) -> bool {
        self.flags & FLAG_SOLD != 0
    }
}

/// A time as the blend counts it: never before 1970.
fn secs(t: i64) -> u128 {
    u128::from(u64::try_from(t).unwrap_or(0))
}

/// The hold clock after `amount` arrives at `now` into a holding of `old_balance` whose clock is
/// `old_stamp`: the entry times averaged by amount, so a top-up moves the clock toward now in
/// proportion to what arrives. An empty holding starts at `now`. Never after `now`.
pub fn blended_stamp(old_stamp: i64, old_balance: u64, amount: u64, now: i64) -> i64 {
    if old_balance == 0 {
        return now;
    }
    let total = u128::from(old_balance) + u128::from(amount);
    let weighted = secs(old_stamp)
        .saturating_mul(u128::from(old_balance))
        .saturating_add(secs(now).saturating_mul(u128::from(amount)));
    let stamp = i64::try_from(weighted / total).unwrap_or(i64::MAX);
    stamp.min(now)
}

/// A receive of `amount` at `now` into a holding of `old_balance`: a fresh holding (empty, or
/// with no record) starts its clock, its round, its paid total and its flags from zero; a funded
/// one blends its clock and keeps the rest.
pub fn on_receive(r: &mut Record, amount: u64, old_balance: u64, now: i64) {
    let fresh = !r.is_set() || old_balance == 0;
    let stamp = if fresh {
        now
    } else {
        blended_stamp(r.stamp, old_balance, amount, now)
    };
    *r = Record {
        version: RECORD_VERSION,
        round: if fresh { 0 } else { r.round },
        stamp,
        amount: old_balance.saturating_add(amount),
        paid: if fresh { 0 } else { r.paid },
        entered_at: if fresh { now } else { r.entered_at },
        flags: if fresh { 0 } else { r.flags },
        measured: if fresh { 0 } else { r.measured },
    };
}

/// A send or a burn that leaves the holding `left` tokens: the clock stays, the amount is cut, and
/// a sell into the pool sets the sold flag. Nothing left: the record is cleared, so the next
/// arrival starts a fresh clock and the holding can close.
pub fn on_send(r: &mut Record, left: u64, sold: bool, now: i64) {
    if left == 0 {
        *r = Record::default();
        return;
    }
    if !r.is_set() {
        // Tokens with no record: they count as just arrived.
        *r = Record {
            version: RECORD_VERSION,
            stamp: now,
            entered_at: now,
            ..Record::default()
        };
    }
    r.amount = left;
    if sold {
        r.flags |= FLAG_SOLD;
    }
}

/// The SOL cut on a sell: `BRAKE_BPS` while the seller's hold clock is under `BRAKE_SECS` (or
/// unknown: a holding with no record counts as just arrived), else none.
pub fn brake_bps(now: i64, stamp: Option<i64>) -> u16 {
    match stamp {
        Some(s) if now.saturating_sub(s) >= BRAKE_SECS => FREE_BPS,
        _ => BRAKE_BPS,
    }
}

/// The hold clock a sell is judged by. The token transfer of a sell runs `before_transfer` before
/// the launchpad asks `quote_cut`, in the same transaction, and that callback sees the holding the
/// tokens really leave; it records that holding's clock and the slot in the hook's state (the
/// relay). `quote_cut` only sees the signer's holding, which a delegate could pick old, so it takes
/// the relay when it was written in this slot and the signer's clock otherwise.
pub fn seller_stamp(
    relay_slot: u64,
    relay_stamp: i64,
    slot: u64,
    actor_stamp: Option<i64>,
) -> Option<i64> {
    if relay_slot != 0 && relay_slot == slot {
        Some(relay_stamp)
    } else {
        actor_stamp
    }
}
