//! T15 — Dip Vault hook: the hold clock, the brake, the relay and the record layout
//! (DIP-VAULT.md items 2 and 5, BUILD ORDER items 5 and 6).

use dipvault_hook::clock::{
    blended_stamp, brake_bps, on_receive, on_send, seller_stamp, Record, BRAKE_BPS, BRAKE_SECS,
    FLAG_SOLD, FREE_BPS, RECORD_VERSION,
};

const NOW: i64 = 1_800_000_000;

#[test]
fn a_record_round_trips_at_its_offsets() {
    let r = Record {
        version: RECORD_VERSION,
        round: 0x0102_0304,
        stamp: -5,
        amount: 7,
        paid: 9,
        entered_at: 11,
        flags: FLAG_SOLD,
        measured: 13,
    };
    let d = r.encode();
    assert_eq!(Record::decode(&d), r);
    assert_eq!(d[0], 1);
    assert_eq!(&d[1..5], &0x0102_0304u32.to_le_bytes());
    assert_eq!(&d[5..13], &(-5i64).to_le_bytes());
    assert_eq!(&d[13..21], &7u64.to_le_bytes());
    assert_eq!(&d[21..29], &9u64.to_le_bytes());
    assert_eq!(&d[29..37], &11i64.to_le_bytes());
    assert_eq!(d[37], 1);
    assert_eq!(&d[38..46], &13u64.to_le_bytes());
    assert_eq!(&d[46..], &[0u8; 18][..]);
    assert_eq!(Record::decode(&[0; 64]), Record::default());
    assert!(!Record::default().is_set());
    assert_eq!(Record::default().stamp_opt(), None);
}

#[test]
fn the_script_reads_the_hooks_record_byte_for_byte() {
    // The companion script decodes the same bytes with its own copy of the layout.
    let r = Record {
        version: 1,
        round: 42,
        stamp: NOW - 100,
        amount: 1_000,
        paid: 55,
        entered_at: NOW - 200,
        flags: FLAG_SOLD,
        measured: 999,
    };
    let s = dipvault_script::record::Record::decode(&r.encode());
    assert_eq!((s.version, s.round, s.stamp, s.amount, s.paid, s.entered_at, s.flags, s.measured), (1, 42, NOW - 100, 1_000, 55, NOW - 200, FLAG_SOLD, 999));
    assert!(s.sold());
}

// ─── the hold clock ──────────────────────────────────────────────────────────

#[test]
fn coins_arriving_in_an_empty_wallet_start_the_clock_now() {
    let mut r = Record::default();
    on_receive(&mut r, 500, 0, NOW);
    assert_eq!((r.stamp, r.amount, r.entered_at, r.round, r.paid, r.flags), (NOW, 500, NOW, 0, 0, 0));
    assert!(r.is_set());
    // A funded wallet with no record (never written) also starts now.
    let mut r = Record::default();
    on_receive(&mut r, 500, 100, NOW);
    assert_eq!((r.stamp, r.amount), (NOW, 600));
}

#[test]
fn a_dust_top_up_barely_moves_the_clock_and_a_huge_one_resets_it() {
    let old = NOW - 30 * 86_400;
    // 1% added to a 30-day wallet: the clock moves about 1% of the way.
    assert_eq!(blended_stamp(old, 1_000_000, 10_000, NOW), old + (30 * 86_400) / 101);
    // The same amount again: halfway.
    assert_eq!(blended_stamp(old, 1_000, 1_000, NOW), old + 15 * 86_400);
    // 1,000x the balance: treated as new (the 30 days shrink to under 45 minutes).
    assert!(NOW - blended_stamp(old, 1_000, 1_000_000, NOW) <= 30 * 86_400 / 1_000);
    // Never after now, whatever the inputs.
    assert!(blended_stamp(NOW + 1_000, 5, 5, NOW) <= NOW);
    assert!(blended_stamp(i64::MAX, u64::MAX, u64::MAX, NOW) <= NOW);
    assert_eq!(blended_stamp(i64::MIN, u64::MAX, u64::MAX, NOW), NOW / 2);
}

#[test]
fn a_top_up_keeps_the_round_the_paid_total_and_the_flags() {
    let mut r = Record {
        version: 1,
        round: 3,
        stamp: NOW - 1_000,
        amount: 100,
        paid: 40,
        entered_at: NOW - 2_000,
        flags: FLAG_SOLD,
        measured: 100,
    };
    on_receive(&mut r, 100, 100, NOW);
    assert_eq!((r.round, r.paid, r.entered_at, r.flags, r.amount, r.measured), (3, 40, NOW - 2_000, FLAG_SOLD, 200, 100));
    assert_eq!(r.stamp, NOW - 500);
}

#[test]
fn sending_out_keeps_the_clock_and_emptying_clears_it() {
    let mut r = Record::default();
    on_receive(&mut r, 1_000, 0, NOW - 5_000);
    on_send(&mut r, 400, false, NOW);
    assert_eq!((r.stamp, r.amount, r.flags), (NOW - 5_000, 400, 0));
    on_send(&mut r, 100, true, NOW);
    assert_eq!((r.stamp, r.amount, r.flags), (NOW - 5_000, 100, FLAG_SOLD));
    on_send(&mut r, 0, true, NOW);
    assert_eq!(r, Record::default());
    // Tokens that leave a holding with no record: what stays counts as just arrived.
    let mut none = Record::default();
    on_send(&mut none, 5, false, NOW);
    assert_eq!((none.version, none.stamp, none.amount, none.entered_at), (1, NOW, 5, NOW));
}

#[test]
fn a_self_transfer_to_a_fresh_wallet_restarts_the_clock() {
    // The receiving wallet is empty: its clock starts now; the sender's clock stays.
    let mut sender = Record::default();
    on_receive(&mut sender, 1_000, 0, NOW - 86_400);
    let mut receiver = Record::default();
    on_send(&mut sender, 500, false, NOW);
    on_receive(&mut receiver, 500, 0, NOW);
    assert_eq!(sender.stamp, NOW - 86_400);
    assert_eq!(receiver.stamp, NOW);
}

// ─── the brake ───────────────────────────────────────────────────────────────

#[test]
fn the_brake_table() {
    assert_eq!(BRAKE_SECS, 600);
    assert_eq!(BRAKE_BPS, 80);
    assert_eq!(FREE_BPS, 0);
    assert_eq!(brake_bps(NOW, Some(NOW)), 80, "just bought");
    assert_eq!(brake_bps(NOW, Some(NOW - 599)), 80);
    assert_eq!(brake_bps(NOW, Some(NOW - 600)), 0, "exactly 10 minutes: free");
    assert_eq!(brake_bps(NOW, Some(NOW - 601)), 0);
    assert_eq!(brake_bps(NOW, Some(NOW - 30 * 86_400)), 0);
    assert_eq!(brake_bps(NOW, Some(NOW + 100)), 80, "a clock in the future counts as new");
    assert_eq!(brake_bps(NOW, None), 80, "no record: counts as just bought");
    assert_eq!(brake_bps(i64::MIN, Some(i64::MAX)), 80, "never panics");
    assert_eq!(brake_bps(i64::MAX, Some(i64::MIN)), 0);
}

#[test]
fn the_launchpad_takes_at_most_the_cap() {
    // hook.json caps sells at 80: the answer never exceeds it, so min(answer, cap) is the answer.
    let cap: u16 = 80;
    for stamp in [None, Some(NOW), Some(NOW - 599), Some(NOW - 600), Some(NOW - 86_400)] {
        let bps = brake_bps(NOW, stamp);
        assert!(bps <= cap);
        assert_eq!(bps.min(cap), bps);
    }
    assert_eq!((dipvault_hook::MAX_BUY_BPS, dipvault_hook::MAX_SELL_BPS), (1, 80), "the constants the Workshop's wizard reads");
    let hook_json = include_str!("../../hook_variants/dipvault/hook.json").replace(' ', "");
    assert!(hook_json.contains("\"maxSellBps\":80"), "{hook_json}");
    assert!(hook_json.contains("\"maxBuyBps\":1"), "the launchpad asks on every buy; the hook answers 0");
}

// ─── the relay and the dust-delegate loophole ────────────────────────────────

#[test]
fn the_relay_wins_in_its_slot_and_the_signer_otherwise() {
    assert_eq!(seller_stamp(100, NOW - 5, 100, Some(NOW - 86_400)), Some(NOW - 5));
    assert_eq!(seller_stamp(100, NOW - 5, 101, Some(NOW - 86_400)), Some(NOW - 86_400));
    assert_eq!(seller_stamp(0, 0, 0, Some(NOW - 86_400)), Some(NOW - 86_400), "slot 0 is never a relay");
    assert_eq!(seller_stamp(100, NOW - 5, 101, None), None);
}

#[test]
fn a_young_holding_sold_through_an_old_dust_delegate_still_pays_the_brake() {
    // A young holding Y (bought a minute ago) approves an old dust wallet D (held 30 days) as its
    // delegate. D signs the sell: the launchpad hands quote_cut D's holding. The sell's token
    // transfer ran before_transfer first, in the same slot, and wrote Y's clock into the relay.
    let slot = 7_777;
    let young = Record { version: 1, stamp: NOW - 60, amount: 1_000, ..Record::default() };
    let dust = Record { version: 1, stamp: NOW - 30 * 86_400, amount: 1, ..Record::default() };
    // before_transfer on the sell: relay = the source holding's clock.
    let (relay_slot, relay_stamp) = (slot, young.stamp_opt().unwrap());
    // quote_cut: the signer's (D's) holding says old, the relay says young.
    let stamp = seller_stamp(relay_slot, relay_stamp, slot, dust.stamp_opt());
    assert_eq!(brake_bps(NOW, stamp), 80, "the brake keys on the real source");
    // Without the relay (another slot) the signer's holding would have dodged it.
    assert_eq!(brake_bps(NOW, seller_stamp(relay_slot, relay_stamp, slot + 1, dust.stamp_opt())), 0);
}

#[test]
fn the_hook_state_is_242_bytes_at_its_offsets() {
    use anchor_lang::prelude::Pubkey;
    use anchor_lang::AccountSerialize;
    use dipvault_hook::HookState;
    let s = HookState {
        version: 1,
        bump: 254,
        mint: Pubkey::new_unique(),
        pool: Pubkey::new_unique(),
        launch: Pubkey::new_unique(),
        creator: Pubkey::new_unique(),
        burned: 1,
        paid_total: 2,
        records: 3,
        relay_slot: 4,
        relay_stamp: -5,
        relay_owner: Pubkey::new_unique(),
        reserved: [0; 32],
    };
    let mut data = Vec::new();
    s.try_serialize(&mut data).unwrap();
    assert_eq!(data.len(), HookState::LEN);
    assert_eq!(HookState::LEN, 242);
    assert_eq!(data[8], 1);
    assert_eq!(data[9], 254);
    assert_eq!(&data[10..42], s.mint.as_ref());
    assert_eq!(&data[42..74], s.pool.as_ref());
    assert_eq!(&data[74..106], s.launch.as_ref());
    assert_eq!(&data[106..138], s.creator.as_ref());
    assert_eq!(&data[138..146], &1u64.to_le_bytes());
    assert_eq!(&data[146..154], &2u64.to_le_bytes());
    assert_eq!(&data[154..162], &3u64.to_le_bytes());
    assert_eq!(&data[162..170], &4u64.to_le_bytes());
    assert_eq!(&data[170..178], &(-5i64).to_le_bytes());
    assert_eq!(&data[178..210], s.relay_owner.as_ref());
    assert_eq!(&data[210..], &[0u8; 32][..]);
    assert_ne!(&data[..8], &bordrless_hook::HOOK_ACCOUNTS_MAGIC[..]);
}
