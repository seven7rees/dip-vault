//! T9: the Dip Vault pair, end to end, in LiteSVM against the Bordrless programs built from the
//! pinned source and the pair's own SBF builds. The story: prepare the hook, a quote-cut config,
//! a native script launch with the payout terms, buys (the hold clock set), an early sell (the
//! brake taken) and a late one (free), fees claimed into the pot, steps: the builder paid, a dip
//! bought and kept, graduation, locked liquidity added, holders measured one week and paid the
//! next. After every step the script's piles add up to the companion's pot.

use std::fs;

use anchor_lang::prelude::{AccountMeta, Pubkey};
use anchor_lang::solana_program::instruction::Instruction;
use anchor_lang::{InstructionData, ToAccountMetas};
use bordrless_companion::client as companion;
use bordrless_companion::constants::{APPROVAL_KIND_SCRIPT, SCRIPT_BOUNTY};
use bordrless_companion::events::{ScriptPayout, ScriptWalletPayout};
use bordrless_companion::instructions::{CreateArgs, CreateScriptArgs, PayoutTermsArgs};
use bordrless_companion::state::{Companion, Split};
use bordrless_core::policy;
use bordrless_hook::token_flags;
use bordrless_launch::client::{self as launch, LaunchKeys};
use bordrless_launch::instructions::CreateConfigArgs;
use bordrless_launch::state::{LaunchRules, QuoteCut};
use bordrless_program_tests::env::Tx;
use bordrless_program_tests::fixture::World;
use bordrless_program_tests::launch::*;
use bordrless_program_tests::timelock::programdata_hash;
use bordrless_swap::events::{LiquidityAdded, Swapped};
use bordrless_program_tests::launch::protocol_lookup_table;
use dipvault_script::params::{DIP_THIRD_BPS, HOLDER_THIRD_BPS, MIN_PAYOUT_LAMPORTS};
use bordrless_token::client as token;
use dipvault_hook::clock::Record;
use dipvault_script::params::BUILDER_WALLET;
use dipvault_script::ScriptState;
use solana_keypair::Keypair;
use solana_signer::Signer;

const HOOK: Pubkey = dipvault_hook::ID;
const SCRIPT: Pubkey = dipvault_script::ID;
const FLAGS: u16 = token_flags::BEFORE_TRANSFER | token_flags::BEFORE_BURN | token_flags::WRITES_HOOK_DATA;
const CREATOR_FEE: u16 = 200;
const BORDRLESS_KEY: Pubkey = bordrless_launch::constants::HOOK_UPGRADE_AUTHORITIES[1];
const WEEK: i64 = 604_800;

fn so(name: &str) -> Vec<u8> {
    let path = format!("{}/../target-sbf/{name}.so", env!("CARGO_MANIFEST_DIR"));
    fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}; build with cargo build-sbf first"))
}

/// A world with the pair loaded as Bordrless deploys it (its upgrade key), the script approved,
/// native launches on.
fn world() -> World {
    let mut w = World::new();
    w.env.svm.add_program(HOOK, &so("dipvault_hook")).expect("load the hook");
    w.env.svm.add_program(SCRIPT, &so("dipvault_script")).expect("load the script");
    w.env.set_upgrade_authority(HOOK, Some(BORDRLESS_KEY));
    w.env.set_upgrade_authority(SCRIPT, Some(BORDRLESS_KEY));
    let deployer = w.env.deployer.insecure_clone();
    let hash = programdata_hash(&w.env, &SCRIPT);
    w.env
        .send(
            &[companion::approve(deployer.pubkey(), SCRIPT, APPROVAL_KIND_SCRIPT, hash, Pubkey::default())],
            &[&deployer],
        )
        .ok();
    let admin = w.admin();
    w.as_admin(launch::set_native_launches(admin.pubkey(), true)).ok();
    w
}

/// The standard `prepare` of the hook for `mint`.
fn prepare_hook(payer: Pubkey, mint: Pubkey) -> Instruction {
    Instruction {
        program_id: HOOK,
        accounts: dipvault_hook::accounts::Prepare {
            payer,
            mint,
            state: Pubkey::find_program_address(&[b"state", mint.as_ref()], &HOOK).0,
            registry: bordrless_hook::hook_accounts_address(&HOOK, &mint).0,
            system_program: anchor_lang::system_program::ID,
        }
        .to_account_metas(None),
        data: dipvault_hook::instruction::Prepare {}.data(),
    }
}

/// A quote-cut config on the hook: 1 bps cap on buys (always asked, answered 0), 80 on sells.
fn config(w: &mut World, creator: &Keypair) -> Pubkey {
    let cfg = Keypair::new();
    let ix = launch::create_quote_cut_config(
        creator.pubkey(),
        cfg.pubkey(),
        CreateConfigArgs {
            rules: LaunchRules::NONE,
            creator_fee_bps: CREATOR_FEE,
            custom_hook: Some(HOOK),
            custom_hook_flags: FLAGS,
            label: "Dip Vault".to_string(),
        },
        0,
        QuoteCut { max_buy_bps: 1, max_sell_bps: 80 },
    );
    w.env.send_paid_by(&[ix], creator, &[&cfg]).ok();
    cfg.pubkey()
}

/// The pair launched by `launcher`: the companion, the hook prepared, the script with its terms
/// and wallet payouts allowed, then the native launch from the config. Answers the mint.
fn launch_pair(w: &mut World, launcher: &Keypair) -> Pubkey {
    let cfg = config(w, launcher);
    let mint = Keypair::new();
    let m = mint.pubkey();
    let ixs = [
        companion::create(
            launcher.pubkey(),
            launcher.pubkey(),
            m,
            CreateArgs {
                split: Split { buyback_bps: 0, holders_bps: 0, beneficiary_bps: 10_000 },
                bounty_bps: 50,
                max_buyback: SOL,
                buyback_interval: 60,
                vest_secs: 0,
                fund: SOL / 2,
            },
        ),
        prepare_hook(launcher.pubkey(), m),
        companion::create_script_v2(
            launcher.pubkey(),
            m,
            CreateScriptArgs {
                split: Split { buyback_bps: 0, holders_bps: 0, beneficiary_bps: 5_000 },
                pot_bps: 5_000,
                program: SCRIPT,
                interval: 300,
                state_len: 320,
                init_data: vec![],
            },
            PayoutTermsArgs { step_bps: 10_000, holding_bps: 10_000, window_bps: 10_000, window_secs: 86_400 },
            vec![],
        ),
        companion::allow_wallet_payouts(m),
    ];
    w.env.send_paid_by(&ixs, launcher, &[&mint]).ok();
    let c = w.launch_config(&cfg);
    let custom = w.custom_hook_accounts(&HOOK, &m);
    let args = World::launch_args("DIPV", c.creator_fee_bps, VQ, c.rules);
    let inner = launch::create_launch_with(
        companion::creator_address(&m),
        m,
        w.env.treasury.pubkey(),
        w.sol,
        policy::LP_FEE_BPS,
        args.clone(),
        Some(cfg),
        Some(&custom),
    );
    let ix = companion::launch_script(launcher.pubkey(), m, &inner, args);
    w.env.send_paid_by(&[ix], launcher, &[&mint]).ok();
    let l = w.launch(&m);
    assert!(l.is_native(), "a native launch");
    assert_eq!(l.custom_hook, Some(HOOK));
    assert!(l.quote_cut().is_some(), "the SOL cut is on");
    m
}

/// A native swap by `trader` (its own wallet pays a buy in lamports), with the SOL cut's accounts.
fn swap_ix(w: &World, trader: &Pubkey, mint: &Pubkey, buy: bool, amount: u64) -> Instruction {
    let slice = w.launch_base_slice(mint, trader, trader, buy);
    let ix = launch::swap_v2_with_base_slice(&w.launch_keys(mint), *trader, *trader, u8::from(buy), amount, 0, slice);
    w.with_quote_cut(ix, mint, trader)
}

fn buy(w: &mut World, trader: &Keypair, mint: &Pubkey, lamports: u64) -> Tx {
    let ixs = [
        token::create_holding(trader.pubkey(), *mint, trader.pubkey()),
        swap_ix(w, &trader.pubkey(), mint, true, lamports),
    ];
    w.env.send_paid_by(&ixs, trader, &[])
}

fn sell(w: &mut World, trader: &Keypair, mint: &Pubkey, amount: u64) -> Tx {
    let ix = swap_ix(w, &trader.pubkey(), mint, false, amount);
    w.env.send_paid_by(&[ix], trader, &[])
}

fn record_of(w: &World, mint: &Pubkey, owner: &Pubkey) -> Record {
    Record::decode(&w.env.hook_data(mint, owner))
}

fn state_of(w: &World, mint: &Pubkey) -> ScriptState {
    w.env.read(&companion::script_state_address(mint))
}

fn companion_of(w: &World, mint: &Pubkey) -> Companion {
    w.env.read(&companion::companion_address(mint))
}

fn hook_state(w: &World, mint: &Pubkey) -> dipvault_hook::HookState {
    w.env.read(&Pubkey::find_program_address(&[b"state", mint.as_ref()], &HOOK).0)
}

/// The accounts a keeper's step brings, as the live docs describe them: the companion's payout
/// accounts, the launch pool's swap accounts for the companion address (the pool, its vaults, the
/// creator's holdings, the hook slice; the companion's own trades are exempt from the SOL cut, so
/// no quote-cut extras), the pool's quote vault (the liquidity add's), the builder wallet, then
/// the candidate holdings with their owners (all writable).
fn step_extras(w: &World, mint: &Pubkey, holders: &[Pubkey]) -> Vec<AccountMeta> {
    let creator = companion::creator_address(mint);
    let l = w.launch(mint);
    let mut extras = companion::payout_accounts(mint, &HOOK);
    let push = |m: AccountMeta, extras: &mut Vec<AccountMeta>| {
        if let Some(e) = extras.iter_mut().find(|e| e.pubkey == m.pubkey) {
            e.is_writable |= m.is_writable;
        } else {
            extras.push(AccountMeta { is_signer: false, ..m });
        }
    };
    let slice = w.launch_base_slice(mint, &creator, &creator, true);
    let swap = launch::swap_v2_with_base_slice(&w.launch_keys(mint), creator, creator, 1, 1, 0, slice);
    for m in swap.accounts {
        push(m, &mut extras);
    }
    push(AccountMeta::new(token::holding_address(&l.quote_mint, &l.pool), false), &mut extras);
    push(AccountMeta::new(BUILDER_WALLET, false), &mut extras);
    for h in holders {
        push(AccountMeta::new(token::holding_address(mint, h), false), &mut extras);
        push(AccountMeta::new(*h, false), &mut extras);
    }
    extras
}

fn step(w: &mut World, cranker: &Keypair, mint: &Pubkey, holders: &[Pubkey]) -> Tx {
    let extras = step_extras(w, mint, holders);
    let ix = companion::script_step(cranker.pubkey(), &LaunchKeys::of(&w.launch(mint)), SCRIPT, extras);
    let tx = w.env.send_paid_by(&[ix], cranker, &[]);
    if tx.result.is_ok() {
        piles_equal_pot(w, mint);
    }
    tx
}

/// Steps until one is a payout turn (no liquidity added), at most `tries`, a step interval apart.
fn payout_turn(w: &mut World, cranker: &Keypair, mint: &Pubkey, holders: &[Pubkey], tries: usize) -> Tx {
    for i in 0..tries {
        let tx = step(w, cranker, mint, holders);
        assert!(tx.result.is_ok(), "{}", tx.logs().join("\n"));
        if tx.events::<LiquidityAdded>().is_empty() || i + 1 == tries {
            assert!(tx.events::<LiquidityAdded>().is_empty(), "no payout turn in {tries} steps");
            return tx;
        }
        w.env.warp(300);
    }
    unreachable!()
}

fn claim(w: &mut World, cranker: &Keypair, mint: &Pubkey) -> Tx {
    w.env
        .send_paid_by(&[companion::claim_fees_script(cranker.pubkey(), *mint, None)], cranker, &[])
}

#[track_caller]
fn piles_equal_pot(w: &World, mint: &Pubkey) {
    let s = state_of(w, mint);
    let c = companion_of(w, mint);
    let total = s.owed + s.dip_pile + s.holder_pile + s.lp_pile;
    assert_eq!(s.last_pot, c.pending_pot, "last_pot is the companion's pot");
    assert_eq!(total, s.last_pot, "the piles add up to the pot");
}

#[test]
fn t9_the_whole_story() {
    let mut w = world();
    let launcher = w.env.funded(50 * SOL);
    let mint = launch_pair(&mut w, &launcher);
    let now0 = w.env.now;
    w.env.warp(31);

    // Three holders buy. Their hold clocks start at the buy.
    let a = w.env.funded(20 * SOL);
    let b = w.env.funded(20 * SOL);
    let c = w.env.funded(20 * SOL);
    buy(&mut w, &a, &mint, SOL).ok();
    let ra = record_of(&w, &mint, &a.pubkey());
    assert!(ra.is_set() && ra.stamp == w.env.now && ra.amount == w.env.holding(&mint, &a.pubkey()), "{ra:?}");
    w.env.warp(60);
    buy(&mut w, &b, &mint, SOL).ok();
    let b_bought_at = w.env.now;
    buy(&mut w, &c, &mint, SOL / 2).ok();
    w.env.warp(120);

    // An early sell pays the brake: exactly 80 bps of the SOL out the launchpad was told
    // (`Swapped.amount_out`), rounded down. A sell after 10 minutes pays nothing.
    let accrued0 = w.launch(&mint).quote_cut_accrued();
    let held_b = w.env.holding(&mint, &b.pubkey());
    let tx = sell(&mut w, &b, &mint, held_b / 10);
    tx.ok();
    let swapped: Swapped = tx.event();
    let accrued1 = w.launch(&mint).quote_cut_accrued();
    let cut = accrued1 - accrued0;
    // The launchpad is told the SOL out after the DEX's 0.3% LP fee (`docs/hooks-v2.md` §3.1), so
    // the cut is 80 bps of a little under the event's `amount_out`: between 79 and 80 bps of it.
    let bps = cut * 10_000 / swapped.amount_out;
    assert!((79..=80).contains(&bps), "the brake is 80 bps of the SOL out: {cut} of {} is {bps} bps", swapped.amount_out);
    assert!(cut > 0);
    let rb = record_of(&w, &mint, &b.pubkey());
    assert!(rb.sold(), "b's record carries the sold flag");
    assert_eq!(rb.stamp, b_bought_at, "a sell keeps the clock");
    w.env.warp(601);
    let held_a = w.env.holding(&mint, &a.pubkey());
    sell(&mut w, &a, &mint, held_a / 10).ok();
    assert_eq!(w.launch(&mint).quote_cut_accrued(), accrued1, "after 10 minutes a sell is free");
    let hs = hook_state(&w, &mint);
    assert_eq!(hs.relay_owner, a.pubkey(), "the relay holds the last seller");

    // The dust-delegate trick: d bought dust long ago (its own clock is old), y buys now and
    // approves d as its delegate; d signs the sell of y's coins. The launchpad hands quote_cut
    // d's holding (old), but the relay carries y's clock (young): the brake is taken.
    let d = w.env.funded(5 * SOL);
    buy(&mut w, &d, &mint, SOL / 100).ok();
    w.env.warp(1_000);
    let y = w.env.funded(5 * SOL);
    buy(&mut w, &y, &mint, SOL).ok();
    let y_holding = token::holding_address(&mint, &y.pubkey());
    let y_held = w.env.holding(&mint, &y.pubkey());
    w.env
        .send_paid_by(&[token::approve(y.pubkey(), y_holding, d.pubkey(), y_held)], &y, &[])
        .ok();
    let mut ix = swap_ix(&w, &d.pubkey(), &mint, false, y_held / 2);
    let d_holding = token::holding_address(&mint, &d.pubkey());
    let slot_index = ix.accounts.iter().position(|m| m.pubkey == d_holding).expect("the trader's base holding");
    ix.accounts[slot_index].pubkey = y_holding;
    let accrued_before = w.launch(&mint).quote_cut_accrued();
    let tx = w.env.send_paid_by(&[ix], &d, &[]);
    assert!(tx.result.is_ok(), "a delegate sell: {}", tx.logs().join("\n"));
    let accrued_after = w.launch(&mint).quote_cut_accrued();
    assert!(accrued_after > accrued_before, "the brake keys on y's clock through the relay");
    assert_eq!(hook_state(&w, &mint).relay_owner, y.pubkey(), "the relay names the real source");
    assert_eq!(record_of(&w, &mint, &d.pubkey()).amount, w.env.holding(&mint, &d.pubkey()), "d's own coins untouched");
    assert!(record_of(&w, &mint, &y.pubkey()).sold(), "y's record carries the sold flag");

    // The fees into the pot; the first step splits them and pays the builder.
    let cranker = w.env.funded(5 * SOL);
    claim(&mut w, &cranker, &mint).ok();
    let pot0 = companion_of(&w, &mint).pending_pot;
    assert!(pot0 > 10 * SCRIPT_BOUNTY, "pot {pot0}");
    w.env.fund(BUILDER_WALLET, SOL);
    let builder_before = w.env.lamports(&BUILDER_WALLET);
    let tx = step(&mut w, &cranker, &mint, &[]);
    assert!(tx.result.is_ok(), "{}", tx.logs().join("\n"));
    let s1 = state_of(&w, &mint);
    assert!(tx.events::<ScriptWalletPayout>().len() == 1, "no builder payout; state {s1:?}\n{}", tx.logs().join("\n"));
    let paid: ScriptWalletPayout = tx.event();
    assert_eq!(paid.to, BUILDER_WALLET);
    assert_eq!(w.env.lamports(&BUILDER_WALLET) - builder_before, paid.lamports);
    // The split, exactly: the pool holds under 5 SOL of real SOL, so the builder's share is 1%;
    // the rest in 3,333 / 3,333 / 3,334; the step's bounty (2,500 for a one-entry answer) comes
    // out of the dip pile; the builder is paid what the terms allow of what is owed.
    assert!(w.launch_pool(&mint).quote_reserve < 5 * SOL);
    let builder = pot0 * 100 / 10_000;
    let rest = pot0 - builder;
    let dip = (rest as u128 * u128::from(DIP_THIRD_BPS) / 10_000) as u64;
    let holders_share = (rest as u128 * u128::from(HOLDER_THIRD_BPS) / 10_000) as u64;
    let lp = rest - dip - holders_share;
    let s1 = state_of(&w, &mint);
    assert_eq!(s1.owed + paid.lamports, builder, "the builder's 1%: paid plus still owed");
    assert_eq!(s1.dip_pile, dip - SCRIPT_BOUNTY / 8, "the dip third less the step's bounty");
    assert_eq!(s1.holder_pile, holders_share);
    assert_eq!(s1.lp_pile, lp);
    assert_eq!(s1.steps, 1);

    // A dip: a whale pumps the price, steps raise the reference toward it (5% a step), then the
    // whale dumps: the price lands well under the reference and the next step buys and keeps.
    let whale = w.env.funded(100 * SOL);
    buy(&mut w, &whale, &mint, 20 * SOL).ok();
    let ref_before = state_of(&w, &mint).ref_price;
    for _ in 0..8 {
        w.env.warp(300);
        let tx = step(&mut w, &cranker, &mint, &[]);
        assert!(tx.result.is_ok(), "{}", tx.logs().join("\n"));
    }
    let s1 = state_of(&w, &mint);
    assert!(s1.ref_price > ref_before, "the reference rose toward the pumped price: {ref_before} -> {}", s1.ref_price);
    w.env.warp(601);
    let held_w = w.env.holding(&mint, &whale.pubkey());
    sell(&mut w, &whale, &mint, held_w).ok();
    claim(&mut w, &cranker, &mint).ok();
    let held_before = w.env.holding(&mint, &companion::creator_address(&mint));
    let pot_before_dip = companion_of(&w, &mint).pending_pot;
    let real_sol_before = w.launch_pool(&mint).quote_reserve;
    let tx = step(&mut w, &cranker, &mint, &[]);
    tx.ok();
    println!("T9 dip-buy step: {} CU, {} instructions, height {}", tx.cu(), tx.trace_len(), tx.max_height());
    let s2 = state_of(&w, &mint);
    assert!(s2.tokens_held > 0, "the dip buy kept coins: {s2:?}\n{}", tx.logs().join("\n"));
    assert_eq!(w.env.holding(&mint, &companion::creator_address(&mint)) - held_before, s2.tokens_held);
    // The dip pile paid for exactly the buy: the pile grew by its third of the new money, then
    // lost the SOL spent and the step's flat bounty (no answer on a trading step).
    let new_money = pot_before_dip - s1.last_pot;
    let dip_share = ((new_money - new_money * 100 / 10_000) as u128 * u128::from(DIP_THIRD_BPS) / 10_000) as u64;
    // The bounty: a flat 20,000 on a step with no answer, 2,500 when it also paid the builder.
    let bounty = if tx.events::<ScriptWalletPayout>().is_empty() { SCRIPT_BOUNTY } else { SCRIPT_BOUNTY / 8 };
    let spent = s1.dip_pile + dip_share - bounty - s2.dip_pile;
    let swapped: Swapped = tx.event();
    assert_eq!(spent, swapped.amount_in, "the dip pile paid the buy: {spent} vs {}", swapped.amount_in);
    assert!(spent <= real_sol_before / 100, "at most 1% of the pool's real SOL: {spent} of {real_sol_before}");
    assert!(spent > 0);
    piles_equal_pot(&w, &mint);

    // Graduation: fresh wallets buy the curve, the last one graduates it.
    let mut buyers = 0;
    loop {
        let l = w.launch(&mint);
        let p = w.launch_pool(&mint);
        let missing = l.graduation_quote.saturating_sub(p.quote_reserve);
        let buyer = w.env.funded(2_000 * SOL);
        if missing <= 20 * SOL {
            let amount = w.crossing_buy_amount(&mint, &buyer.pubkey());
            let ixs = [
                token::create_holding(buyer.pubkey(), mint, buyer.pubkey()),
                swap_ix(&w, &buyer.pubkey(), &mint, true, amount),
                w.graduate_ix(&buyer.pubkey(), &mint),
            ];
            let tx = w.env.send_paid_by(&ixs, &buyer, &[]);
            assert!(tx.result.is_ok(), "graduation: {}", tx.logs().join("\n"));
            break;
        }
        buy(&mut w, &buyer, &mint, (missing / 2).min(200 * SOL)).ok();
        buyers += 1;
        assert!(buyers < 100, "the curve does not fill");
        w.env.warp(5);
    }
    let l = w.launch(&mint);
    assert_eq!(l.status, bordrless_launch::constants::STATUS_GRADUATED);
    assert!(!w.launch_pool(&mint).curve);

    // Locked liquidity: the pot's coins pair with SOL into the launch's own LP holding.
    w.env.warp(601);
    claim(&mut w, &cranker, &mint).ok();
    let lp_before = w.env.holding(&w.launch_pool(&mint).lp_mint, &launch::launch_address(&mint));
    let tx = step(&mut w, &cranker, &mint, &[]);
    assert!(tx.result.is_ok(), "{}", tx.logs().join("\n"));
    println!("T9 liquidity step: {} CU, {} instructions, height {}", tx.cu(), tx.trace_len(), tx.max_height());
    let s3 = state_of(&w, &mint);
    let lp_after = w.env.holding(&w.launch_pool(&mint).lp_mint, &launch::launch_address(&mint));
    assert!(s3.lp_minted > 0 && lp_after == lp_before + s3.lp_minted, "LP minted into the launch's holding: {s3:?}\n{}", tx.logs().join("\n"));
    let added: LiquidityAdded = tx.event();
    assert_eq!(added.lp_minted, s3.lp_minted);
    // The pair is at the pool's ratio before the add (the reserves the event reports less what it
    // added), rounded up on the quote side as the DEX pulls it, and no wrapped SOL is left behind.
    let base_before = added.base_reserve - added.base_amount;
    let quote_before = added.quote_reserve - added.quote_amount;
    // Either every coin offered was paired (quote = ceil(base * Q / B)) or the SOL budget bound
    // the add (base = ceil(quote * B / Q)): the DEX's two branches of `lp_for_deposit`.
    let by_base = dipvault_script::math::ceil_ratio(added.base_amount, quote_before, base_before).unwrap();
    let by_quote = dipvault_script::math::ceil_ratio(added.quote_amount, base_before, quote_before).unwrap();
    assert!(
        added.quote_amount == by_base || added.base_amount == by_quote,
        "the pair is at the pool's ratio: base {} quote {} (ceil by base {by_base}, ceil by quote {by_quote})",
        added.base_amount,
        added.quote_amount
    );
    assert_eq!(w.env.holding(&w.sol, &companion::creator_address(&mint)), 0, "no wrapped SOL stranded");
    assert_eq!(s2.tokens_held - s3.tokens_held, added.base_amount, "the coins held were paired");
    assert!(tx.events::<Swapped>().is_empty(), "the pot held enough coins: no shortfall buy");
    piles_equal_pot(&w, &mint);

    // Holder payouts: a new week, the step measures the holders (touches); the week after, pays.
    let week = |t: i64| (t / WEEK) as u32;
    // Six more holders; e is the ninth,
    // left out of the batches and used later.
    let mut more = Vec::new();
    for _ in 0..6 {
        let h = w.env.funded(5 * SOL);
        buy(&mut w, &h, &mint, SOL / 4).ok();
        more.push(h);
    }
    let e = more.pop().unwrap();
    let to_next_week = WEEK - (w.env.now % WEEK) + 1;
    w.env.warp(to_next_week);
    buy(&mut w, &c, &mint, SOL / 20).ok();
    claim(&mut w, &cranker, &mint).ok();
    let mut holders = vec![a.pubkey(), b.pubkey(), c.pubkey()];
    holders.extend(more.iter().map(|h| h.pubkey()));
    assert_eq!(holders.len(), 8);
    // e is measured in week one with the others, but paid only in the z step of week two.
    let mut measured_set = holders.clone();
    measured_set.push(e.pubkey());
    // How many holders a step fits in one transaction beside the protocol's lookup table: the
    // keeper brings at most keeper.json's perStep and fewer when a step would not fit.
    // The protocol's lookup table as it is on mainnet (4vxVcYLdkqT1rMfGHjAu4kMa9XSEhUdQU5ThVfU5grGQ,
    // read 2026-10-21): the fixture's 18 plus the launchpad, the DEX, the token program and one more.
    let live: [&str; 22] = [
        "69vhpnkYjtiJgdfU7QsA5Ww7V8FWtvPcZ2r4znuyByvq",
        "C2Y3B3hZTesJQqLYrZ7qoaZUoRmwYWh5Qh3MuFxruouE",
        "9cBz2tsg7FwopSsd6HzvtbiL6b7B46eLG436etkB7cyx",
        "6Ztfr97cUewdViXDXdZUsQq4pz7MYdygvK1WijALjZ5q",
        "2XLvgczuVACmvvjfRLLAcLwMzFHutbAAro61zZKP8fsx",
        "EiUK62AP8DwUJWBosf7Ceyih3sGAicZxmsmcBftHzM8n",
        "Dwf4C8tTMYwicp8cU5MYcTtMQVVJTN7W3LcXCQ1UEhMs",
        "3ta59VvLDKLnCKMhYihiC3fQ7W9jPKty6xfx3h5puHGS",
        "EAcZR2i8A6BbnKRdWuyMVDTuiNkD6qc4RkFpxUYJ9Qba",
        "6ogQR9x8o86egutFri8ev6Kx6reTDg8YdwPmvkW29efh",
        "3dfEZLdjRcpTJ4RxPzgkqnqaG2FipRdaHgRW72AL6kqL",
        "5n25iAaXFsjs4UgGQM6fCiRhaQcCVQ1L5BgyRZ7UrmaE",
        "9abhxVTuwck5Ux79act3e2Vkfem7Q4zBwATMctUvHvuE",
        "2repKA1JgDkBo4AffscVee342dcBcH6c2yfpUTRrAiEN",
        "14RJQXPdJfkehit6ezktjd3xujamf8nVSKw2shKamaEH",
        "A49oVhX22ExMwTEtFC6Y8nhBdZ4LJDGhdXLDn4c2f59i",
        "11111111111111111111111111111111",
        "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL",
        "7QuoYRuD9MHzX5EN7qqj62iwNPVqfy74RzmwJ524cgDt",
        "1jcBymHxBjniZDhNPy51Vgm5Nz7pLUdxa9UBHc4TavC",
        "GyzKSnnEu2uN5bBRecE4XYY2enbfR2D2MtxnbJPGy7hk",
        "2XoEWp8cF3kRXg74eVwPAyTFhVCAztn3V88komxAvr22",
    ];
    let table = {
        let mut addresses = protocol_lookup_table(&w);
        for a in live {
            let k: Pubkey = a.parse().unwrap();
            if !addresses.contains(&k) {
                addresses.push(k);
            }
        }
        w.env.put_lookup_table(Pubkey::new_unique(), &addresses)
    };
    let mut fit = 0usize;
    for n in 1..=4 {
        let extras = step_extras(&w, &mint, &holders[..n]);
        let ix = companion::script_step(cranker.pubkey(), &LaunchKeys::of(&w.launch(&mint)), SCRIPT, extras);
        let statics = ix.accounts.iter().filter(|m| !table.addresses.contains(&m.pubkey)).map(|m| m.pubkey).collect::<std::collections::HashSet<_>>().len();
        let size = w.env.v0_size(&[ix], &cranker, &[], &[table.clone()]);
        println!("T9 {n}-holder step: {size} bytes as v0 with the protocol table (limit 1,232), {statics} static keys");
        if size <= 1_232 {
            fit = n;
        }
    }
    assert!(fit >= 1, "at least one holder a step must fit one transaction");
    let keeper = include_str!("../../script_variants/dipvault/keeper.json").replace([' ', '\n'], "");
    assert!(keeper.contains(&format!("\"withSwapAccounts\":{fit}")), "keeper.json perStep must be what fits: {fit}");
    let batch_size = fit;
    // Week one: every holder measured once, `batch_size` a step, touches only.
    let mut touched = 0;
    let mut steps_used = 0;
    while touched < 9 {
        let batch: Vec<Pubkey> = measured_set.iter().copied().filter(|h| record_of(&w, &mint, h).round != week(w.env.now)).take(batch_size).collect();
        let tx = payout_turn(&mut w, &cranker, &mint, &batch, 2);
        let events: Vec<ScriptPayout> = tx.events();
        assert!(!events.is_empty(), "{}", tx.logs().join("\n"));
        assert!(events.iter().all(|p| p.lamports == 0), "week one measures only");
        touched += events.len();
        steps_used += 1;
        assert!(steps_used <= 10);
        w.env.warp(300);
    }
    println!("T9 measuring: {touched} touches in {steps_used} steps");
    let r = week(w.env.now);
    assert!(measured_set.iter().all(|h| record_of(&w, &mint, h).round == r), "the hook recorded the round");
    assert!(measured_set.iter().all(|h| record_of(&w, &mint, h).measured == w.env.holding(&mint, h)), "and the balance measured");
    let s4 = state_of(&w, &mint);
    assert!(s4.w_cur > 0 && s4.round == r && s4.measured == 9, "{s4:?}");
    // The same holders in the same week are not paid or measured again (on a payout turn).
    let tx = payout_turn(&mut w, &cranker, &mint, &holders[..batch_size], 2);
    assert!(tx.events::<ScriptPayout>().is_empty(), "nothing twice in a round");
    // Next week: every holder is paid by weight, four a step, over as many steps as the step
    // budget (20% of the pot a step) needs; none is marked with 0 along the way.
    w.env.warp(WEEK);
    buy(&mut w, &c, &mint, SOL / 10).ok();
    claim(&mut w, &cranker, &mint).ok();
    let before: Vec<u64> = holders.iter().map(|h| w.env.lamports(h)).collect();
    let mut pays: Vec<ScriptPayout> = Vec::new();
    let mut steps_used = 0;
    let week2 = week(w.env.now);
    while pays.len() < 8 {
        let batch: Vec<Pubkey> = holders.iter().copied().filter(|h| record_of(&w, &mint, h).round != week2).take(batch_size).collect();
        assert!(!batch.is_empty());
        let tx = payout_turn(&mut w, &cranker, &mint, &batch, 2);
        let events: Vec<ScriptPayout> = tx.events();
        for p in &events {
            assert!(p.lamports >= MIN_PAYOUT_LAMPORTS, "a holder is paid in full or left for a later step, never marked with 0: {}", p.lamports);
        }
        let s = state_of(&w, &mint);
        println!("T9 pay step {steps_used}: {} events, round {} budget {} paid {} pile {} w_prev {}", events.len(), s.round, s.round_budget, s.round_paid, s.holder_pile, s.w_prev);
        pays.extend(events);
        steps_used += 1;
        assert!(steps_used <= 16, "the eight holders are paid within sixteen steps");
        w.env.warp(300);
    }
    println!("T9 paying: 8 payouts in {steps_used} steps");
    assert_eq!(pays.len(), 8);
    let total: u64 = pays.iter().map(|p| p.lamports).sum();
    for (h, b0) in holders.iter().zip(before) {
        let paid = w.env.lamports(h) - b0;
        assert!(paid >= MIN_PAYOUT_LAMPORTS, "{h} was paid");
        let rec = record_of(&w, &mint, h);
        assert_eq!(rec.paid, paid, "the hook's record matches the SOL paid to {h}");
    }
    // a moves everything to a fresh wallet z: z's clock starts now, this week's round has begun,
    // so z is touched (counted 0) while e, held since before the week, is paid in the same step.
    let z = w.env.funded(2 * SOL);
    let a_held = w.env.holding(&mint, &a.pubkey());
    let z_holding = token::holding_address(&mint, &z.pubkey());
    w.env
        .send_paid_by(
            &[
                token::create_holding(a.pubkey(), mint, z.pubkey()),
                {
                    let slice = dipvault_script::hook_slice(Some(HOOK), &mint, &w.launch(&mint).pool);
                    let hook = token::Hook { program: slice[0].pubkey, signer: slice[1].pubkey };
                    token::transfer_with(a.pubkey(), token::holding_address(&mint, &a.pubkey()), z_holding, mint, Some(hook), slice[2..].to_vec(), a_held)
                },
            ],
            &a,
            &[],
        )
        .ok();
    assert_eq!(record_of(&w, &mint, &a.pubkey()), Record::default(), "a's record is cleared");
    w.env.warp(21_601);
    let e_before = w.env.lamports(&e.pubkey());
    let tx = payout_turn(&mut w, &cranker, &mint, &[z.pubkey(), e.pubkey()], 2);
    let events: Vec<ScriptPayout> = tx.events();
    assert_eq!(events.len(), 2, "z touched, e paid\n{}", tx.logs().join("\n"));
    assert!(w.env.lamports(&e.pubkey()) > e_before, "e, held since before the week, is paid");
    assert_eq!(record_of(&w, &mint, &z.pubkey()).paid, 0, "moved coins earn nothing this week");
    assert_eq!(record_of(&w, &mint, &z.pubkey()).round, week2, "z is marked for the week");
    let s5 = state_of(&w, &mint);
    assert!(s5.holders_paid >= total);
    assert!(s5.round_paid <= s5.round_budget);
    assert!(w.env.now > now0);
}
