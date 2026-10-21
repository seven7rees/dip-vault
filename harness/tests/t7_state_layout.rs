//! T7 — ScriptState layout (SPEC §3, state layout).
//!
//! Verifies:
//!   L1: ScriptState::INIT_SPACE == 120  (field bytes, no discriminator)
//!   L2: Total account space == 8 + 120 == 128
//!   L3: STATE_VERSION == 1
//!   L4: Field byte sizes sum to INIT_SPACE
//!   L5: every field sits at the offset the layout comment in lib.rs gives

use anchor_lang::{AccountSerialize, Space};
use workshop_script::{ScriptState, STATE_VERSION};

fn sample() -> ScriptState {
    use anchor_lang::prelude::Pubkey;
    ScriptState {
        version:      42,
        mint:         Pubkey::new_from_array([0xBBu8; 32]),
        last_pot:     0x_1111_2222_3333_4444_u64,
        owed:         0x_5555_6666_7777_8888_u64,
        paid_total:   0x_9999_AAAA_BBBB_CCCC_u64,
        burned_total: 0x_DDDD_EEEE_FFFF_0000_u64,
        steps:        0x_0101_0202_0303_0404_u64,
        unsplit:      0x_0A0B_0C0D_0E0F_1011_u64,
        ref_price:    0x_0102_0304_0506_0708_090A_0B0C_0D0E_0F10_u128,
        ref_at:       0x_1213_1415_1617_1819_i64,
        last_buy_at:  0x_2021_2223_2425_2627_i64,
        reserved:     [0xCCu8; 7],
    }
}

#[test]
fn script_state_init_space_is_120() {
    assert_eq!(ScriptState::INIT_SPACE, 120, "ScriptState::INIT_SPACE should be 120 (field bytes)");
}

#[test]
fn script_state_total_space_is_128() {
    assert_eq!(8 + ScriptState::INIT_SPACE, 128, "total account space should be 128");
}

#[test]
fn state_version_is_one() {
    assert_eq!(STATE_VERSION, 1u8);
}

#[test]
fn script_state_field_sizes_sum_to_init_space() {
    // version(1) + mint(32) + last_pot(8) + owed(8) + paid_total(8) + burned_total(8) + steps(8)
    // + unsplit(8) + ref_price(16) + ref_at(8) + last_buy_at(8) + reserved(7) = 120
    let sum: usize = 1 + 32 + 8 + 8 + 8 + 8 + 8 + 8 + 16 + 8 + 8 + 7;
    assert_eq!(sum, 120);
    assert_eq!(sum, ScriptState::INIT_SPACE);
}

#[test]
fn script_state_serialized_size_is_128() {
    let mut buf = Vec::new();
    sample().try_serialize(&mut buf).expect("try_serialize");
    assert_eq!(buf.len(), 128, "serialized ScriptState should be 128 bytes, got {}", buf.len());
    assert_eq!(buf[8], 42, "version byte at offset 8");
    assert_eq!(&buf[9..41], &[0xBBu8; 32], "mint at bytes 9..41");
}

#[test]
fn script_state_field_offsets() {
    let mut buf = Vec::new();
    sample().try_serialize(&mut buf).expect("try_serialize");
    let u64_at = |at: usize| u64::from_le_bytes(buf[at..at + 8].try_into().unwrap());
    let i64_at = |at: usize| i64::from_le_bytes(buf[at..at + 8].try_into().unwrap());

    assert_eq!(buf[8], 42, "version at 8");
    assert_eq!(&buf[9..41], &[0xBBu8; 32], "mint at 9..41");
    assert_eq!(u64_at(41), 0x_1111_2222_3333_4444_u64, "last_pot at 41");
    assert_eq!(u64_at(49), 0x_5555_6666_7777_8888_u64, "owed at 49");
    assert_eq!(u64_at(57), 0x_9999_AAAA_BBBB_CCCC_u64, "paid_total at 57");
    assert_eq!(u64_at(65), 0x_DDDD_EEEE_FFFF_0000_u64, "burned_total at 65");
    assert_eq!(u64_at(73), 0x_0101_0202_0303_0404_u64, "steps at 73");
    assert_eq!(u64_at(81), 0x_0A0B_0C0D_0E0F_1011_u64, "unsplit at 81");
    assert_eq!(
        u128::from_le_bytes(buf[89..105].try_into().unwrap()),
        0x_0102_0304_0506_0708_090A_0B0C_0D0E_0F10_u128,
        "ref_price at 89..105"
    );
    assert_eq!(i64_at(105), 0x_1213_1415_1617_1819_i64, "ref_at at 105");
    assert_eq!(i64_at(113), 0x_2021_2223_2425_2627_i64, "last_buy_at at 113");
    assert_eq!(&buf[121..128], &[0xCCu8; 7], "reserved at 121..128");
}
