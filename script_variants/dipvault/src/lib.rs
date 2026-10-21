//! Dip Vault companion script: the pot, split four ways, spent three ways.
//!
//! Every step (the companion calls it with the creator address signing):
//!   1. Check the launch, the hook setup and the companion account; read the pot and the pool.
//!   2. Split the pot's new money: the builder's share scales with the pool's real SOL
//!      (`params::BUILDER_SCALE`, 1% to 10%); the rest goes in three equal piles: dip buys,
//!      holder payouts, locked liquidity. The four piles always add up to the pot.
//!   3. Roll the weekly payout round when a new one has begun.
//!   4. Do one thing, in this order of preference:
//!      - a dip buy: when the price is 8% or more under the script's own reference (`dip.rs`),
//!        at most one every 5 minutes, at most 1% of the pool's real SOL; the coins bought are
//!        kept in the companion's holding for liquidity, never burned and never sold;
//!      - else a locked-liquidity add (`liquidity.rs`), at most every 10 minutes once the launch
//!        has graduated, yielding every other step to holder payouts when both are due: a tranche of the liquidity pile pairs the coins held (buying more only when
//!        it holds too few) with SOL, the LP minted into the launch's own LP holding, which
//!        nobody can spend;
//!      - else the holder payouts (`holders.rs`): each holding the step brings is measured once a
//!        round, and paid from the holder pile by its weight (amount times hold time, 1x at 6
//!        hours, 1.5x at 7 days, 2x at 30 days, 10% more if it never sold) against the round
//!        before, as a payout answer the companion pays and the hook records.
//!   5. The builder is paid what it is owed, within the payout terms, as a wallet answer on any
//!      step that answers no holder payouts.
//!
//! Coins never leave the companion's holding except into the pool as locked liquidity. Money only
//! leaves the pot through the swaps, the liquidity add and the answers. Nothing a
//! cranker passes can change where it goes: the builder wallet is a constant, the holdings are
//! checked against their hook data, and every account is found by key.

#![allow(unexpected_cfgs)]

use anchor_lang::prelude::*;
use anchor_lang::solana_program::instruction::AccountMeta;
use anchor_lang::solana_program::program::invoke;
use bordrless_launch::client as launch_client;
use bordrless_launch::constants::STATUS_GRADUATED;
use bordrless_token::client as token_client;

use crate::math::Piles;
use crate::params::{
    BUILDER_WALLET, DIP, LP_EVERY_SECS, LP_MIN_ADD, LP_PACE_STEPS, PARAMS, PAYOUT_MAX_ENTRIES,
    ROUND_SECS,
};
use crate::record::Record;

pub mod dip;
pub mod holders;
pub mod layout;
pub mod liquidity;
pub mod math;
pub mod params;
pub mod record;

// Placeholder: Bordrless writes the real program id when it builds the source.
declare_id!("EijCJveFpJnzFCP2gyMBtqEJApG5NaAL34EmpokcB9uH");

/// The companion program that calls this script and checks its answers.
pub const COMPANION_ID: Pubkey =
    Pubkey::from_str_const("6ZUM1gWBH9hBBNoJoaVAGwSftyZ6CUda6vUZTW9MsJuo");

/// The Bordrless DEX: the expected owner of the pool account.
pub const DEX_ID: Pubkey =
    Pubkey::from_str_const("GyzKSnnEu2uN5bBRecE4XYY2enbfR2D2MtxnbJPGy7hk");

/// The hook's state seed under the hook program: `["state", mint]`.
pub const HOOK_STATE_SEED: &[u8] = b"state";

// Script state, 320 bytes (keeper.json `stateLen`). Offsets include Anchor's 8-byte discriminator:
//   [0..8]     discriminator
//   [8]        version        u8, always 1
//   [9..41]    mint           Pubkey
//   [41..49]   last_pot       u64  the pot as the companion left it after the previous step
//   [49..57]   owed           u64  the builder's pile: earned, not yet paid
//   [57..65]   dip_pile       u64  the dip buys' pile
//   [65..73]   holder_pile    u64  the holder payouts' pile
//   [73..81]   lp_pile        u64  the locked liquidity's pile
//   [81..89]   paid_builder   u64  lamports paid to the builder wallet so far
//   [89..97]   tokens_held    u64  coins bought and held for liquidity, not yet paired
//   [97..105]  steps          u64  steps run so far
//   [105..121] ref_price      u128 dip reference price (PRICE_SCALE times quote per base); 0 = unset
//   [121..129] ref_at         i64  last time the reference was moved up
//   [129..137] last_buy_at    i64  last time a step bought
//   [137..141] round          u32  the payout round of the last step
//   [141..157] w_prev         u128 total weight measured the round before
//   [157..173] w_cur          u128 total weight measured this round so far
//   [173..181] round_budget   u64  the holder pile at the round's first step
//   [181..189] round_paid     u64  paid to holders this round
//   [189..193] measured       u32  holdings measured this round
//   [193..201] holders_paid   u64  lamports paid to holders, all time
//   [201..209] lp_spent       u64  lamports spent on locked liquidity, all time
//   [209..217] lp_minted      u64  LP minted into the launch's holding, all time
//   [217..225] lp_last_at     i64  when liquidity was last added
//   [225..320] reserved       95 bytes, zero

/// Version byte written into every initialised state.
pub const STATE_VERSION: u8 = 1;

#[account]
#[derive(InitSpace, Debug)]
pub struct ScriptState {
    pub version: u8,
    pub mint: Pubkey,
    pub last_pot: u64,
    pub owed: u64,
    pub dip_pile: u64,
    pub holder_pile: u64,
    pub lp_pile: u64,
    pub paid_builder: u64,
    pub tokens_held: u64,
    pub steps: u64,
    pub ref_price: u128,
    pub ref_at: i64,
    pub last_buy_at: i64,
    pub round: u32,
    pub w_prev: u128,
    pub w_cur: u128,
    pub round_budget: u64,
    pub round_paid: u64,
    pub measured: u32,
    pub holders_paid: u64,
    pub lp_spent: u64,
    pub lp_minted: u64,
    pub lp_last_at: i64,
    pub reserved: [u8; 95],
}

impl ScriptState {
    /// The four piles as the state keeps them.
    pub fn piles(&self) -> Piles {
        Piles {
            owed: self.owed,
            dip: self.dip_pile,
            holders: self.holder_pile,
            lp: self.lp_pile,
        }
    }

    fn set_piles(&mut self, p: Piles) {
        self.owed = p.owed;
        self.dip_pile = p.dip;
        self.holder_pile = p.holders;
        self.lp_pile = p.lp;
    }

    fn round(&self) -> holders::Round {
        holders::Round {
            round: self.round,
            w_prev: self.w_prev,
            w_cur: self.w_cur,
            budget: self.round_budget,
            paid: self.round_paid,
            measured: self.measured,
        }
    }

    fn set_round(&mut self, r: holders::Round) {
        self.round = r.round;
        self.w_prev = r.w_prev;
        self.w_cur = r.w_cur;
        self.round_budget = r.budget;
        self.round_paid = r.paid;
        self.measured = r.measured;
    }
}

#[error_code]
pub enum ScriptError {
    #[msg("the launch is not a native-SOL launch")]
    NotNative,
    #[msg("the launch's hook setup does not match this script's variant")]
    WrongHookSetup,
    #[msg("the launch uses kit rules, which this script does not support")]
    Unsupported,
    #[msg("an account is wrong, missing, or owned by the wrong program")]
    BadAccount,
    #[msg("arithmetic overflow")]
    MathOverflow,
}

#[derive(Accounts)]
#[instruction(mint: Pubkey)]
pub struct CompanionInit<'info> {
    /// The companion's creator address for this mint; only the companion can sign for it.
    #[account(seeds = [b"creator", mint.as_ref()], bump, seeds::program = COMPANION_ID)]
    pub creator: Signer<'info>,
    /// CHECK: the mint, which does not exist yet at init time.
    pub mint_account: UncheckedAccount<'info>,
    /// The state the companion made for this mint: zeroed and owned by this program.
    #[account(zero, seeds = [b"script-state", mint.as_ref()], bump, seeds::program = COMPANION_ID)]
    pub state: Box<Account<'info, ScriptState>>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(mint: Pubkey)]
pub struct CompanionStep<'info> {
    /// The companion's creator address for this mint; it holds the pot and signs the swaps.
    #[account(seeds = [b"creator", mint.as_ref()], bump, seeds::program = COMPANION_ID)]
    pub creator: Signer<'info>,
    /// CHECK: the mint.
    pub mint_account: UncheckedAccount<'info>,
    /// CHECK: the launch for this mint; read and checked in the handler.
    pub launch: UncheckedAccount<'info>,
    /// CHECK: the companion account for this mint; owner and mint checked, pot read by offset.
    pub companion: UncheckedAccount<'info>,
    /// CHECK: the launch's pool; its key is checked against the launch and its owner against the DEX.
    pub pool: UncheckedAccount<'info>,
    #[account(
        mut,
        seeds = [b"script-state", mint.as_ref()],
        bump,
        seeds::program = COMPANION_ID,
        constraint = state.mint == mint && state.version == STATE_VERSION @ ScriptError::BadAccount,
    )]
    pub state: Box<Account<'info, ScriptState>>,
}

/// What one step did: lamports spent on trades, coins bought and kept, coins paired, LP minted.
#[derive(Default)]
struct Done {
    spent: u64,
    kept: u64,
    paired: u64,
    lp_minted: u64,
}

#[program]
pub mod dipvault_script {
    use super::*;

    /// Called once by the companion when the launch is set up, before the mint exists.
    /// `init_data` must be empty: every setting is a constant in `params.rs`.
    pub fn companion_init(
        ctx: Context<CompanionInit>,
        mint: Pubkey,
        init_data: Vec<u8>,
    ) -> Result<()> {
        require!(init_data.is_empty(), ScriptError::BadAccount);
        let s = &mut ctx.accounts.state;
        s.version = STATE_VERSION;
        s.mint = mint;
        s.reserved = [0u8; 95];
        Ok(())
    }

    /// Called by the companion once per step interval.
    pub fn companion_step<'a>(ctx: Context<'a, CompanionStep<'a>>, mint: Pubkey) -> Result<()> {
        let now = Clock::get()?.unix_timestamp;

        // 1. The launch.
        let launch = launch_client::read_launch(&ctx.accounts.launch.to_account_info())?;
        require_keys_eq!(launch.mint, mint, ScriptError::BadAccount);
        require_keys_eq!(launch.pool, ctx.accounts.pool.key(), ScriptError::BadAccount);
        require!(launch.is_native(), ScriptError::NotNative);
        require!(
            launch.custom_hook.is_some() == PARAMS.needs_hook,
            ScriptError::WrongHookSetup
        );
        require!(launch.modules == 0, ScriptError::Unsupported);

        // The companion: check it is this mint's, then read the pot and the launch price.
        let (pot_before, launch_price, launch_at, beneficiary, dev_bag): (u64, u128, i64, Pubkey, u64) = {
            let info = ctx.accounts.companion.to_account_info();
            require_keys_eq!(*info.owner, COMPANION_ID, ScriptError::BadAccount);
            let data = info.try_borrow_data()?;
            let companion_mint =
                layout::read_companion_mint(&data).ok_or(error!(ScriptError::BadAccount))?;
            require_keys_eq!(companion_mint, mint, ScriptError::BadAccount);
            (
                layout::read_pending_pot(&data).ok_or(error!(ScriptError::BadAccount))?,
                layout::read_reference_price(&data).ok_or(error!(ScriptError::BadAccount))?,
                layout::read_reference_at(&data).ok_or(error!(ScriptError::BadAccount))?,
                layout::read_companion_beneficiary(&data).ok_or(error!(ScriptError::BadAccount))?,
                layout::read_dev_bag(&data).ok_or(error!(ScriptError::BadAccount))?,
            )
        };

        // The payout terms, if the step carries them (the companion's, of this mint, writable).
        let terms_key =
            Pubkey::find_program_address(&[b"payout-terms", mint.as_ref()], &COMPANION_ID).0;
        let terms: Option<layout::PayoutTermsView> =
            match ctx.remaining_accounts.iter().find(|a| a.key == &terms_key) {
                Some(info)
                    if *info.owner == COMPANION_ID
                        && info.data_len() >= layout::PAYOUT_TERMS_LEN
                        && info.is_writable =>
                {
                    let data = info.try_borrow_data()?;
                    match (
                        layout::read_payout_terms_mint(&data),
                        layout::read_payout_terms(&data),
                    ) {
                        (Some(m), Some(t)) if m == mint => Some(t),
                        _ => None,
                    }
                }
                _ => {
                    msg!("payout terms not in this step: nobody is paid");
                    None
                }
            };

        // The pool's reserves.
        let reserves: layout::PoolReservesView = {
            let info = ctx.accounts.pool.to_account_info();
            require_keys_eq!(*info.owner, DEX_ID, ScriptError::BadAccount);
            let data = info.try_borrow_data()?;
            let r = layout::read_pool_reserves(&data).ok_or(error!(ScriptError::BadAccount))?;
            require_keys_eq!(r.base_mint, mint, ScriptError::BadAccount);
            r
        };

        // 2. The split: new money since the last step, the builder's share by the pool's real
        // SOL, the rest in three piles. The piles are then made to add up to the pot exactly.
        let mut piles = ctx.accounts.state.piles();
        let new_money = pot_before.saturating_sub(ctx.accounts.state.last_pot);
        piles.split(new_money, math::builder_bps(reserves.quote_reserve));
        piles.reconcile(pot_before);

        // 3. The payout round.
        let mut round = holders::roll(
            ctx.accounts.state.round(),
            holders::round_of(now, ROUND_SECS),
            piles.holders,
        );

        // 4. What this step does. The dip rule first.
        let spot = dip::spot_price(
            reserves.quote_reserve,
            reserves.virtual_quote,
            reserves.base_reserve,
            reserves.virtual_base,
        );
        let mut dip_ref = {
            let s = &ctx.accounts.state;
            dip::Ref {
                price: s.ref_price,
                at: s.ref_at,
                last_buy_at: s.last_buy_at,
            }
        };
        let scale_bps: Option<u64> = match spot {
            Some(price) if PARAMS.dip_only => {
                dip_ref = dip::seed(dip_ref, launch_price, launch_at, price, now);
                dip::decide(&dip_ref, price, now, &DIP)
            }
            Some(_) => Some(math::SCALE_ONE_BPS),
            None => None,
        };
        let pool_share = math::pool_share_bps(
            launch.lp_fee_bps,
            launch.creator_fee_bps,
            launch.rules.holder_fee_buy_bps,
            launch.rules.burn_buy_bps,
            launch.rules.holder_fee_sell_bps,
            launch.rules.burn_sell_bps,
            PARAMS.max_buy_pool_bps,
        );
        let pool_cap = math::bps128(reserves.quote_reserve, pool_share);
        let buy_amount_opt = scale_bps.and_then(|scale| {
            math::buy_amount_scaled(
                piles.dip,
                PARAMS.pace_steps,
                scale,
                PARAMS.min_buy,
                pool_share,
                reserves.quote_reserve,
            )
        });

        // A liquidity add is due every other step once the launch has graduated.
        let graduated = launch.status == STATUS_GRADUATED
            && !reserves.curve
            && reserves.virtual_base == 0
            && reserves.virtual_quote == 0
            && reserves.lp_supply > 0
            && reserves.lp_mint == liquidity::lp_mint_address(&launch.pool);
        let lp_due = graduated
            && now >= ctx.accounts.state.lp_last_at.saturating_add(LP_EVERY_SECS)
            && buy_amount_opt.is_none();
        // Holders the step brings that this round has not measured yet. On a step where both a
        // liquidity add and payouts are due, the payouts take every other step, so neither starves.
        let candidates = terms.is_some()
            && ctx.remaining_accounts.iter().any(|info| {
                *info.owner == bordrless_token::ID
                    && token_client::read_holding(info).is_ok_and(|h| {
                        h.mint == mint && Record::decode(&h.hook_data).round != round.round
                    })
            });
        let payouts_turn = candidates && (!lp_due || ctx.accounts.state.steps % 2 == 1);
        let lp_tranche = if lp_due && !payouts_turn {
            math::lp_tranche(piles.lp, LP_PACE_STEPS, LP_MIN_ADD, pool_cap)
        } else {
            None
        };

        let me = ctx.accounts.creator.key();
        let mint_key = ctx.accounts.mint_account.key();
        let slice = hook_slice(launch.custom_hook, &mint, &launch.pool);
        let keys = launch_client::LaunchKeys::of(&launch);
        let all_accounts: Vec<AccountInfo<'a>> = ctx
            .accounts
            .to_account_infos()
            .into_iter()
            .chain(ctx.remaining_accounts.iter().cloned())
            .collect();
        let held = token_client::holding_address(&mint_key, &me);
        let mut done = Done::default();

        if let Some(amount) = buy_amount_opt {
            // A dip buy: the coins delivered stay in the companion's holding for liquidity.
            let lamports_before = ctx.accounts.creator.lamports();
            let holding_before = holding_amount(&all_accounts, &held)?;
            let min_out = math::min_out(
                amount,
                reserves.base_reserve,
                reserves.virtual_base,
                reserves.quote_reserve,
                reserves.virtual_quote,
                PARAMS.min_out_bps,
            );
            let swap_ix =
                launch_client::swap_v2_with_base_slice(&keys, me, me, 1, amount, min_out, slice.clone());
            invoke(&swap_ix, &all_accounts)?;
            done.spent = lamports_before.saturating_sub(ctx.accounts.creator.lamports());
            done.kept = holding_amount(&all_accounts, &held)?.saturating_sub(holding_before);
            piles.dip = piles.dip.saturating_sub(done.spent);
            // The reference: lowered by a buy toward the price it left.
            if PARAMS.dip_only {
                let price_after = {
                    let info = ctx.accounts.pool.to_account_info();
                    let data = info.try_borrow_data()?;
                    layout::read_pool_reserves(&data).and_then(|r| {
                        dip::spot_price(r.quote_reserve, r.virtual_quote, r.base_reserve, r.virtual_base)
                    })
                };
                dip_ref = dip::after_buy(dip_ref, price_after, now, &DIP);
            }
        } else if let Some(tranche) = lp_tranche {
            // A locked-liquidity add: the coins held (never the dev bag) pair with the tranche; when
            // they are worth less than the tranche, part of it buys more first.
            let lamports_before = ctx.accounts.creator.lamports();
            let holding_before = holding_amount(&all_accounts, &held)?;
            let spendable = holding_before.saturating_sub(dev_bag);
            let worth = math::ceil_ratio(spendable, reserves.quote_reserve, reserves.base_reserve).unwrap_or(u64::MAX);
            let mut buy_spent = 0u64;
            if worth < tranche {
                let buy = liquidity::buy_part(tranche.saturating_sub(worth))
                    .min(pool_cap)
                    .max(PARAMS.min_buy)
                    .min(tranche);
                let min_out = math::min_out(
                    buy,
                    reserves.base_reserve,
                    reserves.virtual_base,
                    reserves.quote_reserve,
                    reserves.virtual_quote,
                    PARAMS.min_out_bps,
                );
                let swap_ix =
                    launch_client::swap_v2_with_base_slice(&keys, me, me, 1, buy, min_out, slice.clone());
                invoke(&swap_ix, &all_accounts)?;
                buy_spent = lamports_before.saturating_sub(ctx.accounts.creator.lamports());
            }
            let coins = holding_amount(&all_accounts, &held)?.saturating_sub(dev_bag);
            let after_buy = {
                let info = ctx.accounts.pool.to_account_info();
                let data = info.try_borrow_data()?;
                layout::read_pool_reserves(&data).ok_or(error!(ScriptError::BadAccount))?
            };
            let quote_budget = tranche.saturating_sub(buy_spent);
            if let Some((base_desired, quote_desired)) = liquidity::pairing(
                coins,
                after_buy.base_reserve,
                after_buy.quote_reserve,
                quote_budget,
            ) {
                let lp_before = holding_amount(&all_accounts, &launch.lp_holding).unwrap_or(0);
                invoke(&liquidity::wrap_sol(me, quote_desired), &all_accounts)?;
                let add_keys = liquidity::AddKeys {
                    provider: me,
                    pool: launch.pool,
                    mint: launch.mint,
                    quote_mint: launch.quote_mint,
                    lp_holding: launch.lp_holding,
                };
                invoke(
                    &liquidity::add_liquidity(&add_keys, base_desired, quote_desired, 1, slice.clone()),
                    &all_accounts,
                )?;
                done.lp_minted = holding_amount(&all_accounts, &launch.lp_holding)
                    .unwrap_or(lp_before)
                    .saturating_sub(lp_before);
            }
            let holding_after = holding_amount(&all_accounts, &held)?;
            done.paired = coins.saturating_sub(holding_after.saturating_sub(dev_bag));
            done.kept = holding_after.saturating_add(done.paired).saturating_sub(holding_before);
            done.spent = lamports_before.saturating_sub(ctx.accounts.creator.lamports());
            piles.lp = piles.lp.saturating_sub(done.spent);
            ctx.accounts.state.lp_last_at = now;
            ctx.accounts.state.lp_spent = ctx.accounts.state.lp_spent.saturating_add(done.spent);
            ctx.accounts.state.lp_minted = ctx.accounts.state.lp_minted.saturating_add(done.lp_minted);
        }
        if PARAMS.dip_only && buy_amount_opt.is_none() {
            if let Some(price) = spot {
                dip_ref = dip::after_wait(dip_ref, price, now, &DIP);
            }
        }

        // 5. The answers. Holder payouts on a step that traded nothing and carries the terms and
        // holdings; else the builder's wallet payout.
        let pot_after = pot_before.saturating_sub(done.spent);
        let mut entries: Vec<(u8, u64, u32)> = Vec::new();
        let mut paid_holders = 0u64;
        let mut pay_builder = 0u64;
        if let Some(ref t) = terms {
            let window_start = math::window_start_now(now, t.window_secs);
            let (window_pot, window_paid) = if t.window_start == window_start {
                (t.window_pot, t.window_paid)
            } else {
                (pot_after, 0u64)
            };
            let budget = math::step_budget(
                pot_before,
                pot_after,
                PARAMS.bounty_bound,
                window_pot,
                window_paid,
                t.step_bps,
                t.window_bps,
            );
            let holding_cap = math::bps64(budget, u64::from(t.holding_bps));
            let rent_min = Rent::get()?.minimum_balance(0);
            if done.spent == 0 {
                let hook = launch.custom_hook.unwrap_or_default();
                let excluded = [
                    ctx.accounts.launch.key(),
                    launch.pool,
                    me,
                    ctx.accounts.companion.key(),
                    crate::ID,
                    ctx.accounts.state.key(),
                    terms_key,
                    hook,
                    beneficiary,
                ];
                let mut step_left = budget;
                for (i, info) in ctx.remaining_accounts.iter().enumerate() {
                    if entries.len() == PAYOUT_MAX_ENTRIES || i > usize::from(u8::MAX) {
                        break;
                    }
                    if *info.owner != bordrless_token::ID {
                        continue;
                    }
                    let Ok(h) = token_client::read_holding(info) else {
                        continue;
                    };
                    if h.mint != mint || !eligible(&h.owner, &excluded) {
                        continue;
                    }
                    let rec = Record::decode(&h.hook_data);
                    if !rec.is_set() || rec.amount != h.amount || rec.round == round.round {
                        continue;
                    }
                    let w = record::weight(&rec, h.amount, now);
                    let mut pay = holders::due(&round, w, piles.holders, step_left, holding_cap);
                    if pay > 0 {
                        // Paid only into an account the companion will take: its owner's, passed
                        // writable, plain, system-owned, not left below rent. Else a touch.
                        let ok = ctx.remaining_accounts.iter().find(|a| *a.key == h.owner).is_some_and(|o| {
                            o.is_writable
                                && !o.executable
                                && *o.owner == anchor_lang::system_program::ID
                                && o.lamports().checked_add(pay).is_some_and(|l| l >= rent_min)
                        });
                        if !ok {
                            pay = 0;
                        }
                    }
                    round.w_cur = round.w_cur.saturating_add(w);
                    round.measured = round.measured.saturating_add(1);
                    round.paid = round.paid.saturating_add(pay);
                    paid_holders = paid_holders.saturating_add(pay);
                    step_left = step_left.saturating_sub(pay);
                    piles.holders = piles.holders.saturating_sub(pay);
                    entries.push((i as u8, pay, round.round));
                }
            }
            if entries.is_empty() && t.wallets_allowed && piles.owed > 0 {
                let candidate = piles.owed.min(budget).min(holding_cap);
                if candidate > 0 {
                    if let Some(wallet) = ctx.remaining_accounts.iter().find(|a| a.key == &BUILDER_WALLET) {
                        let rent = Rent::get()?;
                        let funded = wallet
                            .lamports()
                            .checked_add(candidate)
                            .is_some_and(|l| l >= rent.minimum_balance(wallet.data_len()));
                        if wallet.is_writable
                            && !wallet.executable
                            && *wallet.owner == anchor_lang::system_program::ID
                            && funded
                        {
                            pay_builder = candidate;
                        }
                    }
                }
            }
        }

        // The answer goes last, after every call that sets return data.
        let answered: Option<u64> = if !entries.is_empty() {
            anchor_lang::solana_program::program::set_return_data(&math::encode_payout_answer(&entries));
            Some(entries.len() as u64)
        } else if pay_builder > 0 {
            let tag = u32::try_from(ctx.accounts.state.steps).unwrap_or(u32::MAX);
            anchor_lang::solana_program::program::set_return_data(&math::encode_wallet_answer(
                &BUILDER_WALLET,
                pay_builder,
                tag,
            ));
            Some(1)
        } else {
            None
        };

        // 6. The state: the pot as the companion leaves it (after the payouts and the sender's
        // bounty, which comes out of the piles in order), the piles made to add up to it.
        piles.owed = piles.owed.saturating_sub(pay_builder);
        let bounty = math::step_bounty(pot_after, answered);
        piles.take(bounty);
        let pot_left = pot_after
            .saturating_sub(paid_holders)
            .saturating_sub(pay_builder)
            .saturating_sub(bounty);
        piles.reconcile(pot_left);

        let state = &mut ctx.accounts.state;
        state.set_piles(piles);
        state.set_round(round);
        state.last_pot = pot_left;
        state.paid_builder = state.paid_builder.saturating_add(pay_builder);
        state.holders_paid = state.holders_paid.saturating_add(paid_holders);
        state.tokens_held = state
            .tokens_held
            .saturating_add(done.kept)
            .saturating_sub(done.paired);
        state.steps = state.steps.saturating_add(1);
        if PARAMS.dip_only {
            state.ref_price = dip_ref.price;
            state.ref_at = dip_ref.at;
            state.last_buy_at = dip_ref.last_buy_at;
        }
        Ok(())
    }
}

/// Whether `owner` may be paid: none of `excluded`, not the default key, on the ed25519 curve.
pub fn eligible(owner: &Pubkey, excluded: &[Pubkey]) -> bool {
    *owner != Pubkey::default() && !excluded.contains(owner) && owner.is_on_curve()
}

/// The token-hook slice the swaps, the burns and the liquidity add pass: the hook, the token
/// program's signer for it, then the hook's registry extras in its order: its state (writable),
/// the pool's LP mint (writable), the launch's LP holding (writable). Empty without a hook.
pub fn hook_slice(custom_hook: Option<Pubkey>, mint: &Pubkey, pool: &Pubkey) -> Vec<AccountMeta> {
    match custom_hook {
        Some(hook) => {
            let hook_state = Pubkey::find_program_address(&[HOOK_STATE_SEED, mint.as_ref()], &hook).0;
            let lp_mint = liquidity::lp_mint_address(pool);
            let locked_lp = token_client::holding_address(&lp_mint, &launch_client::launch_address(mint));
            launch_client::custom_hook_slice(
                hook,
                vec![
                    AccountMeta::new(hook_state, false),
                    AccountMeta::new(lp_mint, false),
                    AccountMeta::new(locked_lp, false),
                ],
            )
        }
        None => Vec::new(),
    }
}

/// The token balance of the holding at `key` among `accounts`. A holding that is missing or does
/// not read is an error, not a zero.
fn holding_amount(accounts: &[AccountInfo], key: &Pubkey) -> Result<u64> {
    let info = accounts
        .iter()
        .find(|a| a.key == key)
        .ok_or(error!(ScriptError::BadAccount))?;
    let holding = token_client::read_holding(info).map_err(|_| error!(ScriptError::BadAccount))?;
    Ok(holding.amount)
}
