//! T1 — Offset guard (SPEC F7).
//!
//! Serialises real `Companion`, `PayoutTerms`, and `WalletPayoutAnswer` structs
//! from the programs repo with known field values, then reads them back with the
//! script's byte-offset readers.  Any layout drift causes an immediate failure.
//! The Pool layout is verified by building raw bytes that match the documented
//! layout and asserting both readers (script + strategy) agree.

use anchor_lang::{AccountSerialize, AnchorSerialize};
use anchor_lang::prelude::Pubkey;

use bordrless_companion::state::{
    Companion, GameKind, NativeSol, PayoutTerms, QuoteCutClaimed, Split, WalletPayoutAnswer,
    WalletPayoutEntry,
};
use bordrless_strategy::pool_reserves as strategy_pool_reserves;
use workshop_script::layout::{
    read_companion_mint, read_pending_pot, read_payout_terms, read_pool_reserves,
    read_reference_at, read_reference_price, COMPANION_MINT_OFFSET, COMPANION_POT_OFFSET,
    COMPANION_REFERENCE_AT_OFFSET, COMPANION_REFERENCE_PRICE_OFFSET, PAYOUT_TERMS_LEN,
};

// ─── helpers ─────────────────────────────────────────────────────────────────

fn pubkey(n: u8) -> Pubkey {
    Pubkey::new_from_array([n; 32])
}

/// Serialize an Anchor `#[account]` struct into a fresh Vec (discriminator first).
fn account_bytes<T: AccountSerialize>(acct: &T) -> Vec<u8> {
    let mut buf = Vec::new();
    acct.try_serialize(&mut buf).expect("try_serialize");
    buf
}

// ─── Companion ────────────────────────────────────────────────────────────────

fn make_companion() -> Companion {
    Companion {
        version: 1,
        bump: 200,
        creator_bump: 201,
        mint: pubkey(7),
        beneficiary: pubkey(8),
        split: Split {
            buyback_bps: 5_000,
            holders_bps: 3_000,
            beneficiary_bps: 2_000,
        },
        bounty_bps: 50,
        max_buyback: 1_000_000_000,
        buyback_interval: 300,
        vest_secs: 86400,
        launched: true,
        launched_at: 1_760_000_000,
        dev_tokens: 1_000_000,
        dev_released: 500_000,
        pending_buyback: 111_111,
        pending_holders: 222_222,
        pending_beneficiary: 333_333,
        last_buyback_at: 1_760_000_300,
        claimed_total: 999_999,
        spent_total: 888_888,
        burned_total: 777_777,
        shared_total: 666_666,
        paid_beneficiary_total: 555_555,
        bounties_total: 444_444,
        reference_price: 12_345_678_901_234_567_890_u128,
        reference_at: 1_760_000_600,
        game_hook: Pubkey::default(),
        pot_bps: 0,
        // ← the field under test
        pending_pot: 0x_DEAD_BEEF_CAFE_1234_u64,
        round_secs: 0,
        stranded_burned_at: 0,
        game_kind: GameKind::Lottery,
        pot_locked: 0,
        mode: 2, // MODE_SCRIPT
        quote_cut_claimed: QuoteCutClaimed {
            amount: 42_000,
            stored: true,
        },
        native_sol: NativeSol {
            on: true,
            stored: true,
        },
    }
}

#[test]
fn companion_pending_pot_at_offset_270() {
    let c = make_companion();
    let expected = c.pending_pot;
    let bytes = account_bytes(&c);

    // The discriminator is 8 bytes; pending_pot must be readable at byte 270.
    assert!(
        bytes.len() >= COMPANION_POT_OFFSET + 8,
        "serialized Companion too short: {} bytes, need {}",
        bytes.len(),
        COMPANION_POT_OFFSET + 8
    );

    let got = read_pending_pot(&bytes).expect("read_pending_pot");
    assert_eq!(
        got, expected,
        "pending_pot mismatch: got {got} want {expected}"
    );
}

#[test]
fn companion_mint_at_offset_11() {
    let c = make_companion();
    let expected = c.mint;
    let bytes = account_bytes(&c);

    assert!(
        bytes.len() >= COMPANION_MINT_OFFSET + 32,
        "serialized Companion too short for mint"
    );

    let got = read_companion_mint(&bytes).expect("read_companion_mint");
    assert_eq!(got, expected, "mint mismatch");
}

#[test]
fn companion_reference_price_and_at_offsets() {
    let c = make_companion();
    let bytes = account_bytes(&c);
    assert_eq!(COMPANION_REFERENCE_PRICE_OFFSET, 212);
    assert_eq!(COMPANION_REFERENCE_AT_OFFSET, 228);
    assert_eq!(
        read_reference_price(&bytes).expect("read_reference_price"),
        c.reference_price,
        "reference_price at 212..228"
    );
    assert_eq!(
        read_reference_at(&bytes).expect("read_reference_at"),
        c.reference_at,
        "reference_at at 228..236"
    );
    // Raw bytes, so a drift in either reader or the struct shows up here too.
    assert_eq!(u128::from_le_bytes(bytes[212..228].try_into().unwrap()), c.reference_price);
    assert_eq!(i64::from_le_bytes(bytes[228..236].try_into().unwrap()), c.reference_at);
}

#[test]
fn companion_reference_readers_none_when_short() {
    let c = make_companion();
    let bytes = account_bytes(&c);
    assert!(read_reference_price(&bytes[..227]).is_none());
    assert!(read_reference_at(&bytes[..235]).is_none());
}

#[test]
fn companion_pending_pot_raw_bytes() {
    // Double-check: the 8 bytes at offset 270 decode to DEAD_BEEF_CAFE_1234.
    let c = make_companion();
    let bytes = account_bytes(&c);
    let raw: [u8; 8] = bytes[270..278].try_into().unwrap();
    let val = u64::from_le_bytes(raw);
    assert_eq!(val, 0xDEAD_BEEF_CAFE_1234_u64);
}

// ─── PayoutTerms ──────────────────────────────────────────────────────────────

fn make_payout_terms(wallets_on: bool) -> PayoutTerms {
    let mut reserved = [0u8; 28];
    reserved[0] = if wallets_on { 1 } else { 0 };
    PayoutTerms {
        version: 1,
        bump: 202,
        mint: pubkey(9),
        step_bps: 2_000,
        holding_bps: 10_000,
        window_bps: 5_000,
        window_secs: 86_400,
        window_start: 1_800_000_000_i64,
        window_pot: 9_876_543_210_u64,
        window_paid: 1_234_567_890_u64,
        paid_total: 99_999_999,
        payouts: 42,
        last_paid_at: 1_800_000_123,
        reserved,
    }
}

#[test]
fn payout_terms_len_is_128() {
    let t = make_payout_terms(true);
    let bytes = account_bytes(&t);
    assert_eq!(
        bytes.len(),
        PAYOUT_TERMS_LEN,
        "PayoutTerms serialized to {} bytes, expected {PAYOUT_TERMS_LEN}",
        bytes.len()
    );
}

#[test]
fn payout_terms_fields_round_trip() {
    let t = make_payout_terms(true);
    let bytes = account_bytes(&t);

    let view = read_payout_terms(&bytes).expect("read_payout_terms");

    assert_eq!(view.step_bps, t.step_bps, "step_bps");
    assert_eq!(view.holding_bps, t.holding_bps, "holding_bps");
    assert_eq!(view.window_bps, t.window_bps, "window_bps");
    assert_eq!(view.window_secs, t.window_secs, "window_secs");
    assert_eq!(view.window_start, t.window_start, "window_start");
    assert_eq!(view.window_pot, t.window_pot, "window_pot");
    assert_eq!(view.window_paid, t.window_paid, "window_paid");
    assert!(view.wallets_allowed, "wallets_allowed should be true");
}

#[test]
fn payout_terms_wallets_off() {
    let t = make_payout_terms(false);
    let bytes = account_bytes(&t);
    let view = read_payout_terms(&bytes).expect("read_payout_terms");
    assert!(!view.wallets_allowed, "wallets_allowed should be false");
}

// ─── Pool ─────────────────────────────────────────────────────────────────────
//
// Build raw Pool bytes matching the layout documented in SPEC F7 and layout.rs.
// Two variants: pool without a custom hook (tag 0) and with one (tag 1).
// Both the script's reader and the strategy's reader must return the same values.

fn build_pool_bytes(has_hook: bool) -> Vec<u8> {
    // 8-byte discriminator (arbitrary; real value from sha256)
    let mut buf = vec![0xAAu8; 8];

    // version, bump, lp_mint_bump  [8..10]
    buf.extend_from_slice(&[1u8, 200, 201]);

    // base_mint [11..42]
    buf.extend_from_slice(&[0x11u8; 32]);
    // quote_mint [43..74]
    buf.extend_from_slice(&[0x22u8; 32]);
    // lp_mint [75..106]
    buf.extend_from_slice(&[0x33u8; 32]);
    // base_vault [107..138]
    buf.extend_from_slice(&[0x44u8; 32]);
    // quote_vault [139..170]
    buf.extend_from_slice(&[0x55u8; 32]);

    // hook Option tag [171]
    if has_hook {
        buf.push(1u8); // present
        buf.extend_from_slice(&[0x66u8; 32]); // hook pubkey [172..203]
    } else {
        buf.push(0u8); // absent
    }

    // hook_flags u16
    buf.extend_from_slice(&0x0001u16.to_le_bytes());
    // lp_fee_bps u16 = 30
    buf.extend_from_slice(&30u16.to_le_bytes());
    // protocol_fee_bps u16 = 5
    buf.extend_from_slice(&5u16.to_le_bytes());

    // reserves
    let base_reserve: u64 = 1_000_000_000;
    let quote_reserve: u64 = 2_000_000_000;
    let virtual_base: u64 = 500_000_000;
    let virtual_quote: u64 = 750_000_000;

    buf.extend_from_slice(&base_reserve.to_le_bytes());
    buf.extend_from_slice(&quote_reserve.to_le_bytes());
    buf.extend_from_slice(&virtual_base.to_le_bytes());
    buf.extend_from_slice(&virtual_quote.to_le_bytes());

    buf
}

#[test]
fn pool_without_hook_readers_agree() {
    let bytes = build_pool_bytes(false);
    let script = read_pool_reserves(&bytes).expect("script reader (no hook)");
    let strat = strategy_pool_reserves(&bytes).expect("strategy reader (no hook)");

    assert_eq!(script.base_reserve, strat.base_reserve, "base_reserve");
    assert_eq!(script.quote_reserve, strat.quote_reserve, "quote_reserve");
    assert_eq!(script.virtual_base, strat.virtual_base, "virtual_base");
    assert_eq!(script.virtual_quote, strat.virtual_quote, "virtual_quote");
    assert_eq!(script.lp_fee_bps, strat.lp_fee_bps, "lp_fee_bps");
    assert_eq!(
        script.base_mint.to_bytes(),
        strat.base_mint.to_bytes(),
        "base_mint"
    );

    // Verify known values
    assert_eq!(script.base_reserve, 1_000_000_000);
    assert_eq!(script.quote_reserve, 2_000_000_000);
    assert_eq!(script.lp_fee_bps, 30);
}

#[test]
fn pool_with_hook_readers_agree() {
    let bytes = build_pool_bytes(true);
    let script = read_pool_reserves(&bytes).expect("script reader (with hook)");
    let strat = strategy_pool_reserves(&bytes).expect("strategy reader (with hook)");

    assert_eq!(script.base_reserve, strat.base_reserve, "base_reserve");
    assert_eq!(script.quote_reserve, strat.quote_reserve, "quote_reserve");
    assert_eq!(script.virtual_base, strat.virtual_base, "virtual_base");
    assert_eq!(script.virtual_quote, strat.virtual_quote, "virtual_quote");
    assert_eq!(script.lp_fee_bps, strat.lp_fee_bps, "lp_fee_bps");

    assert_eq!(script.base_reserve, 1_000_000_000);
    assert_eq!(script.quote_reserve, 2_000_000_000);
}

#[test]
fn pool_bad_tag_returns_none() {
    let mut bytes = build_pool_bytes(false);
    bytes[171] = 2; // invalid tag
    assert!(read_pool_reserves(&bytes).is_none(), "script reader should return None");
    assert!(strategy_pool_reserves(&bytes).is_none(), "strategy reader should return None");
}

// ─── WalletPayoutAnswer ───────────────────────────────────────────────────────

#[test]
fn wallet_payout_answer_borsh_layout() {
    // The script uses encode_wallet_answer; this test verifies the byte layout
    // matches what Borsh produces for WalletPayoutAnswer (SPEC F4).
    let to = pubkey(0xAB);
    let lamports: u64 = 500_000;
    let tag: u32 = 77;

    // Borsh-serialize the real struct
    let answer = WalletPayoutAnswer {
        version: 2,
        entries: vec![WalletPayoutEntry { to, lamports, tag }],
    };
    let mut borsh_bytes = Vec::new();
    answer.serialize(&mut borsh_bytes).expect("borsh serialize");

    // Compare layout: version(1) + len(4) + to(32) + lamports(8) + tag(4) = 49 bytes
    assert_eq!(borsh_bytes.len(), 49, "WalletPayoutAnswer Borsh is {len} bytes, want 49", len = borsh_bytes.len());
    assert_eq!(borsh_bytes[0], 2, "version byte");
    assert_eq!(&borsh_bytes[1..5], &1u32.to_le_bytes(), "n=1");
    assert_eq!(&borsh_bytes[5..37], to.as_ref(), "pubkey");
    assert_eq!(&borsh_bytes[37..45], &lamports.to_le_bytes(), "lamports");
    assert_eq!(&borsh_bytes[45..49], &tag.to_le_bytes(), "tag");

    // Must match encode_wallet_answer from the script
    let script_bytes = workshop_script::math::encode_wallet_answer(&to, lamports, tag);
    assert_eq!(borsh_bytes.as_slice(), &script_bytes[..], "encode_wallet_answer mismatch");
}
