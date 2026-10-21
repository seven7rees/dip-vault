//! The hook slice for the swap and the burn, and the builder's accrual across skipped steps.
//!
//! Regression tests for two review findings: a coin without a hook (FUEL) used to panic on its first
//! buy, and a step sent without the payout terms used to erase that interval's builder share.

use anchor_lang::prelude::Pubkey;
use workshop_script::hook_slice;
use workshop_script::math::{accrue, bps128};

fn key(n: u8) -> Pubkey {
    Pubkey::new_from_array([n; 32])
}

#[test]
fn no_hook_gives_an_empty_slice() {
    assert!(hook_slice(None, &key(1)).is_empty());
}

#[test]
fn a_hook_gives_program_signer_and_state() {
    let hook = key(7);
    let mint = key(9);
    let s = hook_slice(Some(hook), &mint);
    assert_eq!(s.len(), 3, "hook, token program's signer for it, one extra");
    assert_eq!(s[0].pubkey, hook);
    assert!(!s[0].is_writable);
    let state = Pubkey::find_program_address(&[b"state", mint.as_ref()], &hook).0;
    assert_eq!(s[2].pubkey, state);
    assert!(s[2].is_writable, "the hook's state is writable, as its registry lists it");
    assert!(!s[2].is_signer);
}

/// Steps with and without terms: the builder still gets `payout_bps` of all the new money.
#[test]
fn skipped_steps_do_not_erase_the_builder_share() {
    let bps: u16 = 2_000;
    let mut owed = 0u64;
    let mut unsplit = 0u64;
    let mut total_new = 0u64;
    // A deterministic pattern: the terms are missing in 3 of every 4 steps.
    for i in 0..1_000u64 {
        let new_money = 10_000_000 + (i * 37_123) % 5_000_000;
        total_new += new_money;
        let on = i % 4 == 3;
        let (o, u) = accrue(owed, unsplit, new_money, on, bps);
        owed = o;
        unsplit = u;
    }
    // One last step with terms on settles whatever is parked.
    let (o, u) = accrue(owed, unsplit, 0, true, bps);
    assert_eq!(u, 0);
    let exact = bps128(total_new, bps);
    // Per-step flooring can only lose a lamport or so per payout step.
    assert!(o <= exact && exact - o <= 1_000, "owed {o} vs exact {exact}");
}

#[test]
fn with_payouts_off_nothing_is_owed() {
    let (owed, unsplit) = accrue(5, 0, 1_000, false, 2_000);
    assert_eq!((owed, unsplit), (5, 1_000));
}

#[test]
fn accrue_never_overflows() {
    let (o, u) = accrue(u64::MAX, u64::MAX, u64::MAX, true, 10_000);
    assert_eq!((o, u), (u64::MAX, 0));
    let (o, u) = accrue(0, u64::MAX, u64::MAX, false, 2_000);
    assert_eq!((o, u), (0, u64::MAX));
}
