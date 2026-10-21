//! T11 — the buy's minimum output (`math::min_out`, `min_out_bps` 9_000) against the real swap's fees.
//!
//! A buy's fees come off the input before the curve: the creator fee with Bordrless's quarter on top
//! of it, then the LP fee (and, to be safe, a further protocol fee on the input). The swap then
//! delivers the curve's output for what is left. These tests build that delivery with the real
//! `bordrless_core::swap_amounts` and check the guard against it:
//!   - at every creator fee up to 7.7% (7.0% with a further 0.25% protocol fee on the input) the buy
//!     delivers at least `min_out`, so no ordinary step is refused;
//!   - at the 10% creator fee, or while the sniper LP fee is high, the buy would deliver less than
//!     `min_out`: the swap fails its slippage check and nothing is spent (the documented limit).

use bordrless_core::{fee_amount, swap_amounts, Reserves};
use workshop_script::math::min_out;
use workshop_script::params::PARAMS;

struct Xorshift64(u64);
impl Xorshift64 {
    fn new(seed: u64) -> Self {
        Self(seed | 1)
    }
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// Tokens the pool delivers for a buy of `amount`, fees as the real programs take them.
fn delivered(amount: u64, creator_bps: u16, lp_bps: u16, protocol_bps: u16, r: &Reserves) -> Option<u64> {
    let creator = fee_amount(amount, creator_bps)?;
    let bordrless_quarter = fee_amount(creator, 2_500)?; // 25% on top of the creator fee
    let received = amount.checked_sub(creator)?.checked_sub(bordrless_quarter)?;
    swap_amounts(true, received, lp_bps, protocol_bps, r).ok().map(|a| a.amount_out)
}

fn floor(amount: u64, r: &Reserves) -> u64 {
    min_out(amount, r.base_reserve, r.virtual_base, r.quote_reserve, r.virtual_quote, PARAMS.min_out_bps)
}

fn reserves(rng: &mut Xorshift64) -> Reserves {
    // A pool with real SOL from 0.05 to 5,000 SOL, tokens and virtual offsets of any size.
    let quote_reserve = 50_000_000 + rng.below(5_000_000_000_000);
    let virtual_quote = rng.below(60_000_000_000) + 1;
    let base_reserve = 1_000_000_000 + rng.below(1_000_000_000_000_000);
    let virtual_base = rng.below(1_000_000_000_000_000);
    Reserves { base_reserve, quote_reserve, virtual_base, virtual_quote }
}

#[test]
fn ordinary_buys_always_clear_the_floor() {
    let mut rng = Xorshift64::new(0xF10);
    // (creator fee, protocol fee on the input): the real launch pools charge no protocol fee on the
    // input of a buy, but a pool that did would still clear up to 7.0%.
    for (creator, protocol) in [(0u16, 25u16), (50, 25), (100, 25), (200, 25), (500, 25), (700, 25), (770, 0)] {
        for _ in 0..20_000 {
            let r = reserves(&mut rng);
            // The buy is at most the pool cap, 1% of the real SOL, and at least min_buy.
            let cap = r.quote_reserve / 100;
            let amount = PARAMS.min_buy + rng.below(cap.saturating_sub(PARAMS.min_buy) + 1);
            // LP fee 0.3%.
            let Some(got) = delivered(amount, creator, 30, protocol, &r) else { continue };
            let want = floor(amount, &r);
            assert!(got >= want, "creator {creator} bps: delivered {got} < floor {want} for {amount} in {r:?}");
        }
    }
}

#[test]
fn the_launch_fee_of_two_percent_clears_with_room() {
    let mut rng = Xorshift64::new(0x2F);
    for _ in 0..20_000 {
        let r = reserves(&mut rng);
        let amount = PARAMS.min_buy + rng.below(r.quote_reserve / 100);
        let Some(got) = delivered(amount, 200, 30, 0, &r) else { continue };
        let ideal = min_out(amount, r.base_reserve, r.virtual_base, r.quote_reserve, r.virtual_quote, 10_000);
        // About 2.8% of the fee-free output is taken: delivered is within 3% of ideal, floor is 10% under it.
        assert!(got as u128 * 10_000 >= ideal as u128 * 9_700, "took more than 3%: {got} of {ideal}");
        assert!(got >= floor(amount, &r));
    }
}

#[test]
fn a_ten_percent_creator_fee_fails_the_floor_and_so_spends_nothing() {
    let mut rng = Xorshift64::new(0x10);
    let mut refused = 0u32;
    for _ in 0..2_000 {
        let r = reserves(&mut rng);
        let amount = PARAMS.min_buy + rng.below(r.quote_reserve / 100);
        let Some(got) = delivered(amount, 1_000, 30, 0, &r) else { continue };
        if got < floor(amount, &r) {
            refused += 1;
        }
    }
    assert!(refused > 1_900, "a 10% creator fee is ~12.7% of the input: the floor must refuse it ({refused})");
}

#[test]
fn the_sniper_fee_at_launch_fails_the_floor_too() {
    let mut rng = Xorshift64::new(0x80);
    for _ in 0..2_000 {
        let r = reserves(&mut rng);
        let amount = PARAMS.min_buy + rng.below(r.quote_reserve / 100);
        // 80% LP fee at the very start of the window.
        let Some(got) = delivered(amount, 200, 8_000, 0, &r) else { continue };
        assert!(got < floor(amount, &r), "an 80% LP fee must not clear the floor");
    }
}
