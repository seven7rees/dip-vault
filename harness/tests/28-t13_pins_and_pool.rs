//! T13 — pins the script's hard-coded addresses to the real programs and the wallet file, and checks
//! the pool-share cap and the buy pile's reserve for the builder.

use anchor_lang::prelude::Pubkey;
use std::str::FromStr;
use workshop_script::math::{accrue, buy_pile, pool_share_bps, pot_after_step};
use workshop_script::params::BUILDER_WALLET;

#[test]
fn companion_id_is_the_real_companion() {
    assert_eq!(workshop_script::COMPANION_ID, bordrless_companion::ID);
}

#[test]
fn dex_id_is_the_real_dex() {
    assert_eq!(workshop_script::DEX_ID, bordrless_swap::ID);
}

#[test]
fn builder_wallet_is_the_wallet_file() {
    let file = include_str!("../../WALLET.txt").trim();
    assert_eq!(BUILDER_WALLET, Pubkey::from_str(file).expect("WALLET.txt is a pubkey"));
}

#[test]
fn builder_wallet_is_a_normal_wallet_key() {
    // On the ed25519 curve, so a person's keypair can sign for it (a PDA could not).
    assert!(BUILDER_WALLET.is_on_curve(), "BUILDER_WALLET must be an ordinary wallet address");
}

// ─── pool share ──────────────────────────────────────────────────────────────

#[test]
fn pool_share_follows_the_fees_up_to_the_cap() {
    // lp 30 + creator c on each side: (2 * (30 + c)) / 4.
    assert_eq!(pool_share_bps(30, 200, 0, 0, 0, 0, 100), 100, "2%: 115 capped to 100");
    assert_eq!(pool_share_bps(30, 100, 0, 0, 0, 0, 100), 65, "1%");
    assert_eq!(pool_share_bps(30, 50, 0, 0, 0, 0, 100), 40, "0.5%");
    assert_eq!(pool_share_bps(30, 0, 0, 0, 0, 0, 100), 15, "no creator fee");
    assert_eq!(pool_share_bps(0, 0, 0, 0, 0, 0, 100), 0, "no fees: no buy");
    assert_eq!(pool_share_bps(u16::MAX, u16::MAX, u16::MAX, u16::MAX, u16::MAX, u16::MAX, 100), 100, "no overflow");
}

// ─── the buy pile keeps the builder's parked share ───────────────────────────

#[test]
fn parked_share_is_reserved_from_the_pile() {
    // Payouts off for a step: 1_000_000 of new money parks in `unsplit`, nothing is owed yet.
    let (owed, unsplit) = accrue(0, 0, 1_000_000, false, 1_000);
    assert_eq!((owed, unsplit), (0, 1_000_000));
    // The builder's 10% of it (100_000) must stay out of the pile.
    let pot = 1_000_000u64;
    assert_eq!(buy_pile(pot, owed, unsplit, 1_000), 900_000);
    // What is owed comes out too.
    assert_eq!(buy_pile(pot, 50_000, unsplit, 1_000), 850_000);
    assert_eq!(buy_pile(10, 50_000, 0, 1_000), 0, "never underflows");
    // The step after that has the terms: the parked money is split and nothing stays parked.
    let (owed2, unsplit2) = accrue(owed, unsplit, 0, true, 1_000);
    assert_eq!((owed2, unsplit2), (100_000, 0));
}

#[test]
fn the_pot_left_after_a_step_follows_the_companion() {
    // No payout: the flat 20_000 bounty. A payout of one entry: 2_500. A pot too small to cover it: none.
    assert_eq!(pot_after_step(1_000_000, 0), 980_000);
    assert_eq!(pot_after_step(1_000_000, 100_000), 897_500);
    assert_eq!(pot_after_step(10_000, 0), 10_000);
    assert_eq!(pot_after_step(0, 0), 0);
}
