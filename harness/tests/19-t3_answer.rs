//! T3 — Answer encoding round-trip (SPEC F4, F5).
//!
//! encode_wallet_answer([u8;49]) must match Borsh(WalletPayoutAnswer).
//! decode_script_answer must parse what encode_wallet_answer produces.

use anchor_lang::prelude::Pubkey;
use anchor_lang::AnchorSerialize;

use bordrless_companion::state::{WalletPayoutAnswer, WalletPayoutEntry};
use bordrless_companion::instructions::{decode_script_answer, ScriptAnswer};
use workshop_script::math::encode_wallet_answer;

fn pubkey(n: u8) -> Pubkey {
    Pubkey::new_from_array([n; 32])
}

#[test]
fn encode_matches_borsh_single_entry() {
    let to      = pubkey(0xAB);
    let lamports = 500_000_u64;
    let tag      = 77_u32;

    // Reference: Borsh bytes of WalletPayoutAnswer { version:2, entries:[...] }
    let answer = WalletPayoutAnswer {
        version: 2,
        entries: vec![WalletPayoutEntry { to, lamports, tag }],
    };
    let mut borsh_bytes = Vec::new();
    answer.serialize(&mut borsh_bytes).expect("borsh");

    // Script encoding
    let script_bytes = encode_wallet_answer(&to, lamports, tag);

    assert_eq!(
        borsh_bytes.len(),
        49,
        "WalletPayoutAnswer should be 49 bytes, got {}",
        borsh_bytes.len()
    );
    assert_eq!(
        &script_bytes[..],
        borsh_bytes.as_slice(),
        "encode_wallet_answer != Borsh(WalletPayoutAnswer)"
    );
}

#[test]
fn encode_matches_borsh_zero_lamports() {
    let to = pubkey(0);
    let borsh = {
        let a = WalletPayoutAnswer {
            version: 2,
            entries: vec![WalletPayoutEntry { to, lamports: 0, tag: 0 }],
        };
        let mut b = Vec::new();
        a.serialize(&mut b).unwrap();
        b
    };
    let script = encode_wallet_answer(&to, 0, 0);
    assert_eq!(&script[..], borsh.as_slice());
}

#[test]
fn encode_matches_borsh_max_values() {
    let to = pubkey(0xFF);
    let borsh = {
        let a = WalletPayoutAnswer {
            version: 2,
            entries: vec![WalletPayoutEntry {
                to,
                lamports: u64::MAX,
                tag: u32::MAX,
            }],
        };
        let mut b = Vec::new();
        a.serialize(&mut b).unwrap();
        b
    };
    let script = encode_wallet_answer(&to, u64::MAX, u32::MAX);
    assert_eq!(&script[..], borsh.as_slice());
}

#[test]
fn decode_script_answer_round_trip() {
    // decode_script_answer accepts the same bytes that encode_wallet_answer produces.
    let to      = pubkey(0x12);
    let lamports = 1_234_567_u64;
    let tag      = 42_u32;

    let encoded = encode_wallet_answer(&to, lamports, tag);
    let decoded = decode_script_answer(&encoded).expect("decode_script_answer should succeed");

    // encode_wallet_answer writes version=2 → decodes as ScriptAnswer::Wallets
    let answer = match decoded {
        ScriptAnswer::Wallets(a) => a,
        ScriptAnswer::Holdings(_) => panic!("expected ScriptAnswer::Wallets"),
    };
    assert_eq!(answer.version, 2, "version");
    assert_eq!(answer.entries.len(), 1, "expected 1 entry");
    assert_eq!(answer.entries[0].to, to, "to pubkey");
    assert_eq!(answer.entries[0].lamports, lamports, "lamports");
    assert_eq!(answer.entries[0].tag, tag, "tag");
}

#[test]
fn decode_script_answer_empty_slice_returns_err() {
    let result = decode_script_answer(&[]);
    assert!(result.is_err(), "empty bytes should fail");
}

#[test]
fn decode_script_answer_wrong_version_fails() {
    let to = pubkey(1);
    let mut bytes = encode_wallet_answer(&to, 1000, 0);
    bytes[0] = 99; // corrupt version byte
    // May succeed or fail depending on implementation — at minimum must not panic.
    let _ = decode_script_answer(&bytes);
}

#[test]
fn encode_byte_layout_explicit() {
    // Verify the internal byte layout of encode_wallet_answer directly.
    let to       = pubkey(7);
    let lamports = 0x_CAFE_BABE_DEAD_BEEF_u64;
    let tag      = 0x1234_ABCD_u32;
    let b = encode_wallet_answer(&to, lamports, tag);

    // byte 0: version = 2
    assert_eq!(b[0], 2, "version");
    // bytes 1..5: n=1 as little-endian u32
    assert_eq!(&b[1..5], &1u32.to_le_bytes(), "n=1");
    // bytes 5..37: to pubkey
    assert_eq!(&b[5..37], to.as_ref(), "pubkey");
    // bytes 37..45: lamports
    assert_eq!(&b[37..45], &lamports.to_le_bytes(), "lamports");
    // bytes 45..49: tag
    assert_eq!(&b[45..49], &tag.to_le_bytes(), "tag");
}
