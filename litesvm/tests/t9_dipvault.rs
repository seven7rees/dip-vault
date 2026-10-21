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
use bordrless_swap::events::LiquidityAdded;
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
            PayoutTermsArgs { step_bps: 2_000, holding_bps: 10_000, window_bps: 10_000, window_secs: 86_400 },
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

/// The accounts a keeper's step brings: the companion's payout accounts, the swap's accounts
/// (the pool, its vaults, the creator's holdings, the hook slice, the SOL cut's), the liquidity
/// add's, the builder wallet, then the candidate holdings with their owners (all writable).
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
    for m in swap_ix(w, &creator, mint, true, 1).accounts {
        push(m, &mut extras);
    }
    let keys = dipvault_script::liquidity::AddKeys {
        provider: creator,
        pool: l.pool,
        mint: *mint,
        quote_mint: l.quote_mint,
        lp_holding: l.lp_holding,
    };
    let slice = dipvault_script::hook_slice(Some(HOOK), mint, &l.pool);
    for m in dipvault_script::liquidity::add_liquidity(&keys, 1, 1, 1, slice).accounts {
        push(m, &mut extras);
    }
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
    w.env.send_paid_by(&[ix], cranker, &[])
}

/// Steps until one answers holder payouts (at most `tries`, a step interval apart): a step whose
/// turn is the liquidity add yields the next one to the holders.
fn payout_step(w: &mut World, cranker: &Keypair, mint: &Pubkey, holders: &[Pubkey], tries: usize) -> (Tx, Vec<ScriptPayout>) {
    for i in 0..tries {
        let tx = step(w, cranker, mint, holders);
        assert!(tx.result.is_ok(), "{}", tx.logs().join("\n"));
        let events: Vec<ScriptPayout> = tx.events();
        if !events.is_empty() || i + 1 == tries {
            return (tx, events);
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
    buy(&mut w, &c, &mint, SOL / 2).ok();

    // An early sell pays the brake: the launch's SOL-cut total grows. A late one pays nothing.
    let accrued0 = w.launch(&mint).quote_cut_accrued();
    let held_b = w.env.holding(&mint, &b.pubkey());
    sell(&mut w, &b, &mint, held_b / 10).ok();
    let accrued1 = w.launch(&mint).quote_cut_accrued();
    assert!(accrued1 > accrued0, "the brake took SOL: {accrued0} -> {accrued1}");
    let rb = record_of(&w, &mint, &b.pubkey());
    assert!(rb.sold(), "b's record carries the sold flag");
    assert_eq!(rb.stamp, w.env.now - 0, "a sell keeps the clock (b bought this second)");
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
    assert!(w.env.lamports(&BUILDER_WALLET) > builder_before);
    let s1 = state_of(&w, &mint);
    assert!(s1.dip_pile > 0 && s1.holder_pile > 0 && s1.lp_pile > 0, "{s1:?}");
    assert_eq!(s1.steps, 1);
    piles_equal_pot(&w, &mint);

    // A dip: a whale pumps the price, steps raise the reference toward it (5% a step), then the
    // whale dumps: the price lands well under the reference and the next step buys and keeps.
    let whale = w.env.funded(100 * SOL);
    buy(&mut w, &whale, &mint, 20 * SOL).ok();
    for _ in 0..8 {
        w.env.warp(300);
        let tx = step(&mut w, &cranker, &mint, &[]);
        assert!(tx.result.is_ok(), "{}", tx.logs().join("\n"));
    }
    let s1 = state_of(&w, &mint);
    w.env.warp(601);
    let held_w = w.env.holding(&mint, &whale.pubkey());
    sell(&mut w, &whale, &mint, held_w).ok();
    claim(&mut w, &cranker, &mint).ok();
    let held_before = w.env.holding(&mint, &companion::creator_address(&mint));
    let tx = step(&mut w, &cranker, &mint, &[]);
    tx.ok();
    println!("T9 dip-buy step: {} CU, {} instructions, height {}", tx.cu(), tx.trace_len(), tx.max_height());
    let s2 = state_of(&w, &mint);
    assert!(s2.tokens_held > 0, "the dip buy kept coins: {s2:?}\n{}", tx.logs().join("\n"));
    assert_eq!(w.env.holding(&mint, &companion::creator_address(&mint)) - held_before, s2.tokens_held);
    assert!(s2.dip_pile < s1.dip_pile + (s2.last_pot - s1.last_pot), "the dip pile paid for it");
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
    let _added: LiquidityAdded = tx.event();
    assert!(s3.tokens_held < s2.tokens_held || s3.tokens_held == 0, "coins paired");
    piles_equal_pot(&w, &mint);

    // Holder payouts: a new week, the step measures the holders (touches); the week after, pays.
    let week = |t: i64| (t / WEEK) as u32;
    // Five more holders, so a paying step carries eight entries (PAYOUT_MAX_ENTRIES).
    let mut more = Vec::new();
    for _ in 0..5 {
        let h = w.env.funded(5 * SOL);
        buy(&mut w, &h, &mint, SOL / 4).ok();
        more.push(h);
    }
    let to_next_week = WEEK - (w.env.now % WEEK) + 1;
    w.env.warp(to_next_week);
    buy(&mut w, &c, &mint, SOL / 10).ok();
    claim(&mut w, &cranker, &mint).ok();
    buy(&mut w, &c, &mint, SOL / 20).ok();
    claim(&mut w, &cranker, &mint).ok();
    let mut holders = vec![a.pubkey(), b.pubkey(), c.pubkey()];
    holders.extend(more.iter().map(|h| h.pubkey()));
    let (tx, touches) = payout_step(&mut w, &cranker, &mint, &holders, 2);
    assert_eq!(touches.len(), 8, "eight holders measured\n{}", tx.logs().join("\n"));
    println!("T9 measuring step (8 touches): {} CU, {} instructions in the trace, height {}", tx.cu(), tx.trace_len(), tx.max_height());
    assert!(touches.iter().all(|p| p.lamports == 0), "week one measures only");
    let r = week(w.env.now);
    assert!(holders.iter().all(|h| record_of(&w, &mint, h).round == r), "the hook recorded the round");
    let s4 = state_of(&w, &mint);
    assert!(s4.w_cur > 0 && s4.round == r, "{s4:?}");
    piles_equal_pot(&w, &mint);
    // The same holders in the same week are not paid or measured again.
    w.env.warp(300);
    let tx = step(&mut w, &cranker, &mint, &holders);
    assert!(tx.result.is_ok(), "{}", tx.logs().join("\n"));
    assert!(tx.events::<ScriptPayout>().is_empty(), "nothing twice in a round");
    // Next week: paid by weight.
    w.env.warp(WEEK);
    buy(&mut w, &c, &mint, SOL / 10).ok();
    claim(&mut w, &cranker, &mint).ok();
    let before: Vec<u64> = holders.iter().map(|h| w.env.lamports(h)).collect();
    let (tx, pays) = payout_step(&mut w, &cranker, &mint, &holders, 2);
    assert_eq!(pays.len(), 8, "{}", tx.logs().join("\n"));
    println!("T9 paying step (8 payouts): {} CU, {} instructions in the trace, height {}", tx.cu(), tx.trace_len(), tx.max_height());
    let total: u64 = pays.iter().map(|p| p.lamports).sum();
    assert!(total > 0, "week two pays: {:?}", pays.iter().map(|p| p.lamports).collect::<Vec<_>>());
    for (h, b0) in holders.iter().zip(before) {
        let paid = w.env.lamports(h) - b0;
        let rec = record_of(&w, &mint, h);
        assert_eq!(rec.paid, paid, "the hook's record matches the SOL paid to {h}");
    }
    // a moves everything to a fresh wallet z: z's clock starts now, this week's round has begun,
    // so z is not measured or paid this week (not even touched: it is brought again next week).
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
    let tx = step(&mut w, &cranker, &mint, &[z.pubkey()]);
    assert!(tx.result.is_ok(), "{}", tx.logs().join("\n"));
    assert!(tx.events::<ScriptPayout>().is_empty(), "moved coins earn nothing this week");
    assert_eq!(record_of(&w, &mint, &z.pubkey()).round, 0, "z is not even marked: it waits for next week");
    let s5 = state_of(&w, &mint);
    assert_eq!(s5.holders_paid, total);
    assert!(s5.round_paid <= s5.round_budget);
    piles_equal_pot(&w, &mint);
    assert!(w.env.now > now0);
}
