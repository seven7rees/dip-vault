//! T16 — Dip Vault script: the pot split (builder scale, the three thirds) and its invariant
//! (DIP-VAULT.md item 4): the four piles always add up to the pot, every lamport in and out
//! accounted for.

use dipvault_script::math::{builder_bps, lp_tranche, step_bounty, Piles};
use dipvault_script::params::{BUILDER_MAX_BPS, BUILDER_SCALE, DIP_THIRD_BPS, HOLDER_THIRD_BPS, LP_THIRD_BPS};

struct Xorshift64(u64);
impl Xorshift64 {
    fn new(s: u64) -> Self { Self(s | 1) }
    fn next(&mut self) -> u64 {
        let mut x = self.0; x ^= x << 13; x ^= x >> 7; x ^= x << 17; self.0 = x; x
    }
}

const SOL: u64 = 1_000_000_000;

#[test]
fn the_builder_scale_is_1_to_10_percent_rising_with_pool_sol() {
    assert_eq!(builder_bps(0), 100, "1% on an empty pool");
    assert_eq!(builder_bps(5 * SOL - 1), 100);
    assert_eq!(builder_bps(5 * SOL), 250);
    assert_eq!(builder_bps(20 * SOL), 500);
    assert_eq!(builder_bps(50 * SOL), 750);
    assert_eq!(builder_bps(100 * SOL), 1_000, "10% from 100 SOL");
    assert_eq!(builder_bps(u64::MAX), 1_000);
    assert_eq!(BUILDER_MAX_BPS, BUILDER_SCALE[BUILDER_SCALE.len() - 1].bps);
    let mut last = (0u64, 0u16);
    for s in BUILDER_SCALE.iter() {
        assert!(s.from_lamports >= last.0 && s.bps >= last.1, "steps rise");
        assert!(s.bps >= 100 && s.bps <= 1_000, "1% to 10%");
        last = (s.from_lamports, s.bps);
    }
    assert_eq!(BUILDER_SCALE[0].from_lamports, 0);
}

#[test]
fn the_thirds_add_up() {
    assert_eq!(u32::from(DIP_THIRD_BPS) + u32::from(HOLDER_THIRD_BPS) + u32::from(LP_THIRD_BPS), 10_000);
    let mut p = Piles::default();
    p.split(1_000_000, 1_000);
    assert_eq!(p.owed, 100_000, "10% to the builder");
    assert_eq!(p.dip, 299_970);
    assert_eq!(p.holders, 299_970);
    assert_eq!(p.lp, 300_060, "the liquidity third takes the rounding");
    assert_eq!(p.total(), 1_000_000, "nothing lost");
    let mut q = Piles::default();
    q.split(7, 100);
    assert_eq!(q.total(), 7, "tiny amounts: every lamport lands somewhere");
    let mut m = Piles::default();
    m.split(u64::MAX, 1_000);
    assert_eq!(m.total(), u64::MAX, "no overflow");
}

#[test]
fn take_and_reconcile_keep_the_sum_exact() {
    let mut p = Piles { owed: 10, dip: 5, holders: 7, lp: 3 };
    assert_eq!(p.take(6), 0);
    assert_eq!(p, Piles { owed: 10, dip: 0, holders: 7, lp: 2 }, "dip first, then lp");
    assert_eq!(p.take(100), 81, "what the piles cannot cover is returned");
    assert_eq!(p.total(), 0);
    let mut p = Piles { owed: 1, dip: 2, holders: 3, lp: 4 };
    p.reconcile(20);
    assert_eq!(p, Piles { owed: 1, dip: 12, holders: 3, lp: 4 }, "a surplus goes to the dip pile");
    p.reconcile(8);
    assert_eq!(p.total(), 8, "a shortfall comes out in order");
    assert_eq!(p, Piles { owed: 1, dip: 0, holders: 3, lp: 4 });
    p.reconcile(0);
    assert_eq!(p.total(), 0);
}

/// The step as the script accounts it: inflow, a split, one spend out of one pile, payouts out of
/// the holder pile, the builder out of owed, the sender's bounty, then reconcile against the pot
/// the companion keeps. The invariant: piles.total() == pot after every step, and no pile ever
/// pays what it does not hold.
#[test]
fn n_step_simulation_keeps_the_piles_equal_to_the_pot() {
    let mut rng = Xorshift64::new(0xD1_5EED);
    let mut pot = 0u64;
    let mut piles = Piles::default();
    let (mut in_total, mut out_total) = (0u64, 0u64);
    for step in 0..5_000u64 {
        let inflow = rng.next() % 50_000_000;
        pot += inflow;
        in_total += inflow;
        let quote_reserve = rng.next() % (200 * SOL);
        let new_money = pot - piles.total();
        piles.split(new_money, builder_bps(quote_reserve));
        piles.reconcile(pot);
        assert_eq!(piles.total(), pot, "after the split, step {step}");
        // One action.
        let mut spent = 0u64;
        let mut paid_holders = 0u64;
        let mut pay_builder = 0u64;
        let mut entries = None;
        match rng.next() % 4 {
            0 => { spent = piles.dip.min(rng.next() % 10_000_000); piles.dip -= spent; }
            1 => { spent = piles.lp.min(rng.next() % 10_000_000); piles.lp -= spent; }
            2 => {
                let n = 1 + rng.next() % 8;
                for _ in 0..n {
                    let pay = piles.holders.min(rng.next() % 1_000_000);
                    piles.holders -= pay;
                    paid_holders += pay;
                }
                entries = Some(n);
            }
            _ => {
                pay_builder = piles.owed.min(rng.next() % 2_000_000);
                piles.owed -= pay_builder;
                if pay_builder > 0 { entries = Some(1); }
            }
        }
        let pot_after = pot - spent;
        let bounty = step_bounty(pot_after, entries);
        let short = piles.take(bounty);
        assert_eq!(short, 0, "the bounty is always covered when the companion pays it");
        let pot_left = pot_after - paid_holders - pay_builder - bounty;
        piles.reconcile(pot_left);
        assert_eq!(piles.total(), pot_left, "after the step, step {step}");
        out_total += spent + paid_holders + pay_builder + bounty;
        pot = pot_left;
    }
    assert_eq!(in_total, out_total + pot, "every lamport in is out or still in the pot");
}

#[test]
fn the_liquidity_tranche_is_bounded() {
    assert_eq!(lp_tranche(0, 48, 2_000_000, SOL), None);
    assert_eq!(lp_tranche(1_999_999, 48, 2_000_000, SOL), None, "below the least add");
    assert_eq!(lp_tranche(2_000_000, 48, 2_000_000, SOL), Some(2_000_000), "a small pile goes whole");
    assert_eq!(lp_tranche(480_000_000, 48, 2_000_000, SOL), Some(10_000_000), "1/48 of the pile");
    assert_eq!(lp_tranche(480_000_000, 48, 2_000_000, 1_000_000), Some(2_000_000), "twice the pool cap");
    assert_eq!(lp_tranche(480_000_000, 48, 2_000_000, 500_000), None, "a pool too thin to take the least add");
    assert_eq!(lp_tranche(u64::MAX, 0, 2_000_000, u64::MAX), Some(u64::MAX), "no panic on a zero pace");
}

#[test]
fn the_lp_the_dex_would_mint() {
    use dipvault_script::liquidity::lp_for;
    assert_eq!(lp_for(100, 200, 1_000, 2_000, 10_000), 1_000, "a tenth of the pool: a tenth of the supply");
    assert_eq!(lp_for(100, 100, 1_000, 2_000, 10_000), 500, "the short side binds");
    assert_eq!(lp_for(1, 1, u64::MAX, u64::MAX, 1), 0, "too small: no LP, so no add");
    assert_eq!(lp_for(1, 1, 0, 1, 1), 0);
    assert_eq!(lp_for(u64::MAX, u64::MAX, 1, 1, u64::MAX), u64::MAX, "no overflow");
    assert_eq!(dipvault_script::params::LP_PACE_STEPS, 24, "24 adds, 10 minutes apart: about 4 hours a pile");
}
