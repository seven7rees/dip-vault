//! T14 — DIP's dip constants (item name DIP, internal key fuel2) (trigger 8%, a step every 5 minutes) against the first FUEL's (3%, a
//! step a minute), on the same random price paths. The point of the second variant is that "buys only
//! dips" stays true in choppy markets: far fewer buys when the price only wobbles, the same buys when
//! the price really falls. Uses the real `dip.rs`; prices are a plain path, not a pool.

use workshop_script::dip::{after_buy, after_wait, decide, seed, Ref};
use workshop_script::math::buy_amount_scaled;
use workshop_script::params::{Dip, DIP};

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
    /// About a standard normal: twelve uniforms summed, less six.
    fn gauss(&mut self) -> f64 {
        (0..12).map(|_| (self.next() >> 11) as f64 / (1u64 << 53) as f64).sum::<f64>() - 6.0
    }
}

fn v2() -> Dip {
    Dip { trigger_bps: 800, step_secs: 300, ..DIP }
}

const BASE: u128 = 1_000_000_000_000;

/// Run a per-minute price path; return (buys, fraction of a pile spent at 1/pace per scaled buy).
fn run(path: &[u128], d: &Dip, pace: u32) -> (u32, f64) {
    let mut state = Ref::default();
    let (mut buys, mut left) = (0u32, 1.0f64);
    for (minute, &spot) in path.iter().enumerate() {
        let now = minute as i64 * 60;
        if now % d.step_secs != 0 {
            continue;
        }
        state = seed(state, 0, 0, spot, now);
        let scale = decide(&state, spot, now, d);
        let amount = scale.and_then(|s| buy_amount_scaled(1_000_000_000_000, pace, s, 500_000, 100, u64::MAX));
        if let Some(a) = amount {
            buys += 1;
            left *= 1.0 - a as f64 / 1_000_000_000_000.0;
            state = after_buy(state, Some(spot), now, d);
        } else {
            state = after_wait(state, spot, now, d);
        }
    }
    (buys, 1.0 - left)
}

fn walk(sigma_pct: f64, seed: u64) -> Vec<u128> {
    let mut rng = Xorshift64::new(seed);
    let mut price = BASE as f64;
    (0..1_440).map(|_| { price = (price * (1.0 + sigma_pct / 100.0 * rng.gauss())).max(1.0); price as u128 }).collect()
}

fn avg(sigma: f64, d: &Dip, pace: u32) -> f64 {
    (0..20).map(|k| run(&walk(sigma, 0x5EED + k), d, pace).0 as f64).sum::<f64>() / 20.0
}

#[test]
fn a_wobbly_market_is_bought_far_less_than_before() {
    for sigma in [0.5, 1.0, 2.0] {
        let (old, new) = (avg(sigma, &DIP, 240), avg(sigma, &v2(), 48));
        assert!(new * 2.0 < old, "sigma {sigma}%/min: fuel2 (the DIP item) bought {new} a day against FUEL's {old}");
    }
    assert!(avg(0.5, &v2(), 48) < 8.0, "a quiet market should be bought a handful of times a day");
}

#[test]
fn a_real_crash_is_still_bought() {
    // -40% over 30 minutes, then flat.
    let mut path = vec![BASE; 30];
    path.extend((1..=30u128).map(|i| BASE - BASE * 4 * i / 300));
    path.extend(std::iter::repeat(BASE * 6 / 10).take(600));
    let (buys, spent) = run(&path, &v2(), 48);
    assert!(buys >= 5, "a 40% crash was bought only {buys} times");
    assert!(spent > 0.15 && spent < 0.6, "a 40% crash spent {:.0}% of the pile", spent * 100.0);
}

#[test]
fn a_sharp_wick_is_bought_once() {
    // -20% in 3 minutes, back to flat over 10.
    let mut path = vec![BASE; 30];
    path.extend((1..=3u128).map(|i| BASE - BASE * 2 * i / 30));
    path.extend((1..=10u128).map(|i| BASE * 8 / 10 + BASE * 2 * i / 100));
    path.extend(std::iter::repeat(BASE).take(60));
    let (buys, _) = run(&path, &v2(), 48);
    assert_eq!(buys, 1, "a one-off wick should be bought once, got {buys}");
}

#[test]
fn a_pump_and_partial_dump_is_not_bought() {
    // x4 in 10 minutes, then back to x1.5: the price is still far above its old mark.
    let mut path = vec![BASE; 30];
    path.extend((1..=10u128).map(|i| BASE + BASE * 3 * i / 10));
    path.extend((1..=10u128).map(|i| BASE * 4 - BASE * 25 * i / 100));
    path.extend(std::iter::repeat(BASE * 3 / 2).take(300));
    let (buys, _) = run(&path, &v2(), 48);
    assert_eq!(buys, 0, "price never fell under its mark, yet it bought {buys} times");
}
