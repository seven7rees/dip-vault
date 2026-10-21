//! Buy-and-burn companion script with a builder payout.
//!
//! Every step (the companion calls it with the creator address signing):
//!   1. Check the launch, the hook setup and the companion account.
//!   2. Look for the PayoutTerms account; without it the builder share waits (see `unsplit`).
//!   3. Read the pool's reserves for the buy size and the minimum output.
//!   4. Split the pot's new money: `payout_bps` of it is owed to the builder wallet.
//!   5. Size one buy from the rest of the pot (the buy pile) and, when there is one,
//!      swap on the launch pool and burn exactly the tokens the swap delivered. A dip-only variant
//!      (`PARAMS.dip_only`) buys only while the price is under its own reference (`dip.rs`).
//!   6. Pay the builder up to what is owed, within the payout terms, as a wallet answer
//!      set as return data after every other call.
//!
//! Money only leaves the pot through the swap and the answer. Nothing a cranker passes can change
//! where it goes: the builder wallet is a constant, and every account is found by key.

#![allow(unexpected_cfgs)]

use anchor_lang::prelude::*;
use anchor_lang::solana_program::program::invoke;
use bordrless_launch::client as launch_client;
use bordrless_token::client as token_client;

use crate::params::{BUILDER_WALLET, DIP, PARAMS};

pub mod dip;
pub mod layout;
pub mod math;
pub mod params;

// Placeholder: Bordrless writes the real program id when it builds the source.
declare_id!("ScRiPtXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX");

/// The companion program that calls this script and checks its answers.
pub const COMPANION_ID: Pubkey =
    Pubkey::from_str_const("6ZUM1gWBH9hBBNoJoaVAGwSftyZ6CUda6vUZTW9MsJuo");

/// The Bordrless DEX: the expected owner of the pool account.
pub const DEX_ID: Pubkey =
    Pubkey::from_str_const("GyzKSnnEu2uN5bBRecE4XYY2enbfR2D2MtxnbJPGy7hk");

// Script state, 128 bytes (keeper.json `stateLen`). Offsets include Anchor's 8-byte discriminator:
//   [0..8]    discriminator
//   [8]       version        u8, always 1
//   [9..41]   mint           Pubkey
//   [41..49]  last_pot       u64 LE  pot after the previous step's own spending and payout
//   [49..57]  owed           u64 LE  builder share earned, not yet paid
//   [57..65]  paid_total     u64 LE  lamports paid to the builder wallet so far
//   [65..73]  burned_total   u64 LE  tokens burned so far
//   [73..81]  steps          u64 LE  steps run so far
//   [81..89]  unsplit        u64 LE  new pot money not yet split because payouts were off that step
//   [89..105] ref_price      u128 LE dip reference price (PRICE_SCALE times quote per base); 0 = unset
//   [105..113] ref_at        i64 LE  last time the reference was moved up
//   [113..121] last_buy_at   i64 LE  last time a step bought
//   [121..128] reserved      7 bytes, zero (the dip fields stay zero when `dip_only` is false)

/// Version byte written into every initialised state.
pub const STATE_VERSION: u8 = 1;

#[account]
#[derive(InitSpace)]
pub struct ScriptState {
    /// Layout version; checked against `STATE_VERSION` on every step.
    pub version: u8,
    /// The mint this state belongs to; checked on every step.
    pub mint: Pubkey,
    /// Pot after the previous step's spending and payout; new money is `pot - last_pot`.
    pub last_pot: u64,
    /// Builder share earned and not yet paid (lamports).
    pub owed: u64,
    /// Total paid to the builder wallet.
    pub paid_total: u64,
    /// Total tokens burned.
    pub burned_total: u64,
    /// Steps run.
    pub steps: u64,
    /// New pot money that arrived in a step where payouts were off (terms account missing or
    /// wallet payouts not allowed); it is split in the first step where payouts are on.
    pub unsplit: u64,
    /// The dip reference price; 0 until the first step of a dip-only variant.
    pub ref_price: u128,
    /// When the dip reference was last moved up.
    pub ref_at: i64,
    /// When a step last bought.
    pub last_buy_at: i64,
    /// Zero.
    pub reserved: [u8; 7],
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
    #[msg("a buy is due but the mint account is not writable in this step")]
    MintNotWritable,
    #[msg("arithmetic overflow")]
    MathOverflow,
}

#[derive(Accounts)]
#[instruction(mint: Pubkey)]
pub struct CompanionInit<'info> {
    /// The companion's creator address for this mint; only the companion can sign for it.
    #[account(
        seeds = [b"creator", mint.as_ref()],
        bump,
        seeds::program = COMPANION_ID,
    )]
    pub creator: Signer<'info>,
    /// CHECK: the mint, which does not exist yet at init time.
    pub mint_account: UncheckedAccount<'info>,
    /// The state the companion made for this mint: zeroed and owned by this program.
    #[account(
        zero,
        seeds = [b"script-state", mint.as_ref()],
        bump,
        seeds::program = COMPANION_ID,
    )]
    pub state: Box<Account<'info, ScriptState>>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(mint: Pubkey)]
pub struct CompanionStep<'info> {
    /// The companion's creator address for this mint; it holds the pot and signs the swap and burn.
    #[account(
        seeds = [b"creator", mint.as_ref()],
        bump,
        seeds::program = COMPANION_ID,
    )]
    pub creator: Signer<'info>,
    /// CHECK: the mint; must be writable in the step for the burn (keeper.json `mintWritable`).
    pub mint_account: UncheckedAccount<'info>,
    /// CHECK: the launch for this mint; read and checked in the handler.
    pub launch: UncheckedAccount<'info>,
    /// CHECK: the companion account for this mint; owner and mint checked, pot read by byte offset.
    pub companion: UncheckedAccount<'info>,
    /// CHECK: the launch's pool; its key is checked against the launch and its owner against the DEX.
    pub pool: UncheckedAccount<'info>,
    /// This script's state for the mint.
    #[account(
        mut,
        seeds = [b"script-state", mint.as_ref()],
        bump,
        seeds::program = COMPANION_ID,
        constraint = state.mint == mint && state.version == STATE_VERSION @ ScriptError::BadAccount,
    )]
    pub state: Box<Account<'info, ScriptState>>,
}

#[program]
pub mod workshop_script {
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
        s.last_pot = 0;
        s.owed = 0;
        s.paid_total = 0;
        s.burned_total = 0;
        s.steps = 0;
        s.unsplit = 0;
        s.ref_price = 0;
        s.ref_at = 0;
        s.last_buy_at = 0;
        s.reserved = [0u8; 7];
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

        // 2. The companion: check it is this mint's, then read the pot.
        let (pot_before, launch_price, launch_at): (u64, u128, i64) = {
            let info = ctx.accounts.companion.to_account_info();
            require_keys_eq!(*info.owner, COMPANION_ID, ScriptError::BadAccount);
            let data = info.try_borrow_data()?;
            let companion_mint =
                layout::read_companion_mint(&data).ok_or(error!(ScriptError::BadAccount))?;
            require_keys_eq!(companion_mint, mint, ScriptError::BadAccount);
            let pot = layout::read_pending_pot(&data).ok_or(error!(ScriptError::BadAccount))?;
            let launch_price =
                layout::read_reference_price(&data).ok_or(error!(ScriptError::BadAccount))?;
            let launch_at =
                layout::read_reference_at(&data).ok_or(error!(ScriptError::BadAccount))?;
            (pot, launch_price, launch_at)
        };

        // 3. The payout terms, if the step carries them. Payouts are on only when the account is
        // the companion's, of this mint, and wallet payouts are allowed.
        let terms_key =
            Pubkey::find_program_address(&[b"payout-terms", mint.as_ref()], &COMPANION_ID).0;
        let terms: Option<layout::PayoutTermsView> = {
            match ctx.remaining_accounts.iter().find(|a| a.key == &terms_key) {
                None => {
                    msg!("payout terms not in this step: builder share waits");
                    None
                }
                Some(info) => {
                    if *info.owner != COMPANION_ID || info.data_len() < layout::PAYOUT_TERMS_LEN {
                        msg!("payout terms account is not the companion's: builder share waits");
                        None
                    } else if !info.is_writable {
                        // The companion refuses an answer unless the terms are writable.
                        msg!("payout terms account is not writable: builder share waits");
                        None
                    } else {
                        let data = info.try_borrow_data()?;
                        match (
                            layout::read_payout_terms_mint(&data),
                            layout::read_payout_terms(&data),
                        ) {
                            (Some(m), Some(t)) if m == mint && t.wallets_allowed => Some(t),
                            _ => {
                                msg!("wallet payouts are off for this coin: builder share waits");
                                None
                            }
                        }
                    }
                }
            }
        };
        let payouts_on = terms.is_some();

        // 4. The pool's reserves.
        let reserves: layout::PoolReservesView = {
            let info = ctx.accounts.pool.to_account_info();
            require_keys_eq!(*info.owner, DEX_ID, ScriptError::BadAccount);
            let data = info.try_borrow_data()?;
            let r = layout::read_pool_reserves(&data).ok_or(error!(ScriptError::BadAccount))?;
            require_keys_eq!(r.base_mint, mint, ScriptError::BadAccount);
            r
        };

        // 5. New money in the pot since the last step. With payouts on, `payout_bps` of it (plus
        // any money that arrived while they were off) becomes owed to the builder; with payouts
        // off it is parked in `unsplit` so a cranker cannot erase the builder's share by leaving
        // the terms out of a step.
        {
            let state = &mut ctx.accounts.state;
            let new_money = pot_before.saturating_sub(state.last_pot);
            let (owed, unsplit) = math::accrue(
                state.owed,
                state.unsplit,
                new_money,
                payouts_on,
                PARAMS.payout_bps,
            );
            state.owed = owed;
            state.unsplit = unsplit;
        }
        // The buy pile leaves out what is owed to the builder and the builder's share of parked money.
        let pile = math::buy_pile(
            pot_before,
            ctx.accounts.state.owed,
            ctx.accounts.state.unsplit,
            PARAMS.payout_bps,
        );

        // 6. Buy size. A dip-only variant buys only while the price is under its reference, and
        // buys more the deeper the dip; any other variant buys its plain pace every step.
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
        let scale_bps: Option<u64> = if PARAMS.dip_only {
            match spot {
                Some(price) => {
                    dip_ref = dip::seed(dip_ref, launch_price, launch_at, price, now);
                    dip::decide(&dip_ref, price, now, &DIP)
                }
                None => None,
            }
        } else {
            Some(math::SCALE_ONE_BPS)
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
        let buy_amount_opt = scale_bps.and_then(|scale| {
            math::buy_amount_scaled(
                pile,
                PARAMS.pace_steps,
                scale,
                PARAMS.min_buy,
                pool_share,
                reserves.quote_reserve,
            )
        });

        // 7. Swap, then burn exactly what the swap delivered.
        let me = ctx.accounts.creator.key();
        let mint_key = ctx.accounts.mint_account.key();
        let mut spent: u64 = 0;
        let mut bought: u64 = 0;

        if let Some(amount) = buy_amount_opt {
            // A burn needs the mint writable. Fail before anything is spent.
            require!(
                ctx.accounts.mint_account.is_writable,
                ScriptError::MintNotWritable
            );

            let slice = hook_slice(launch.custom_hook, &mint);

            let keys = launch_client::LaunchKeys::of(&launch);

            let all_accounts: Vec<AccountInfo<'a>> = ctx
                .accounts
                .to_account_infos()
                .into_iter()
                .chain(ctx.remaining_accounts.iter().cloned())
                .collect();

            let lamports_before = ctx.accounts.creator.lamports();
            let held = token_client::holding_address(&mint_key, &me);
            let holding_before = holding_amount(&all_accounts, &held)?;

            let min_out_tokens = math::min_out(
                amount,
                reserves.base_reserve,
                reserves.virtual_base,
                reserves.quote_reserve,
                reserves.virtual_quote,
                PARAMS.min_out_bps,
            );

            let swap_ix = launch_client::swap_v2_with_base_slice(
                &keys,
                me,
                me,
                1,
                amount,
                min_out_tokens,
                slice.clone(),
            );
            invoke(&swap_ix, &all_accounts)?;

            spent = lamports_before.saturating_sub(ctx.accounts.creator.lamports());
            bought = holding_amount(&all_accounts, &held)?.saturating_sub(holding_before);

            if bought > 0 {
                let (hook_struct, extras_vec) = match slice.as_slice() {
                    [prog, signer, rest @ ..] => (
                        Some(token_client::Hook {
                            program: prog.pubkey,
                            signer: signer.pubkey,
                        }),
                        rest.to_vec(),
                    ),
                    _ => (None, vec![]),
                };
                let burn_ix = token_client::burn_with(
                    me,
                    held,
                    mint_key,
                    hook_struct,
                    extras_vec,
                    bought,
                );
                invoke(&burn_ix, &all_accounts)?;
            }
        }

        // The dip reference: lowered by a buy toward the price it left, raised by a wait.
        if PARAMS.dip_only {
            if let Some(price) = spot {
                dip_ref = if buy_amount_opt.is_some() {
                    let price_after = {
                        let info = ctx.accounts.pool.to_account_info();
                        let data = info.try_borrow_data()?;
                        layout::read_pool_reserves(&data).and_then(|r| {
                            dip::spot_price(
                                r.quote_reserve,
                                r.virtual_quote,
                                r.base_reserve,
                                r.virtual_base,
                            )
                        })
                    };
                    dip::after_buy(dip_ref, price_after, now, &DIP)
                } else {
                    dip::after_wait(dip_ref, price, now, &DIP)
                };
            }
        }

        // 8. The builder payout: at most what is owed and at most what the companion will accept
        // (the same budget the companion computes; an answer over it fails the whole step).
        let pot_after = pot_before.saturating_sub(spent);
        let mut pay: u64 = 0;

        if let Some(ref t) = terms {
            if ctx.accounts.state.owed > 0 {
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
                let cap = math::bps64(budget, u64::from(t.holding_bps));
                let candidate = ctx.accounts.state.owed.min(budget).min(cap);

                if candidate > 0 {
                    if let Some(wallet) = ctx
                        .remaining_accounts
                        .iter()
                        .find(|a| a.key == &BUILDER_WALLET)
                    {
                        // The companion refuses the whole step unless the wallet is writable,
                        // plain, system-owned and ends rent-exempt, so check all of that first.
                        let rent = Rent::get()?;
                        let funded = wallet
                            .lamports()
                            .checked_add(candidate)
                            .map_or(false, |l| l >= rent.minimum_balance(wallet.data_len()));
                        if wallet.is_writable
                            && !wallet.executable
                            && *wallet.owner == anchor_lang::system_program::ID
                            && funded
                        {
                            pay = candidate;
                        }
                    }
                }
            }
        }

        // 9. The answer goes last, after every call that sets return data.
        if pay > 0 {
            let tag = u32::try_from(ctx.accounts.state.steps).unwrap_or(u32::MAX);
            let answer = math::encode_wallet_answer(&BUILDER_WALLET, pay, tag);
            anchor_lang::solana_program::program::set_return_data(&answer);
        }

        // 10. State.
        // The pot as the companion leaves it, after the payout and the sender's bounty.
        let pot_left = math::pot_after_step(pot_after, pay);
        let state = &mut ctx.accounts.state;
        state.owed = state.owed.saturating_sub(pay);
        state.last_pot = pot_left;
        state.paid_total = state.paid_total.saturating_add(pay);
        state.burned_total = state.burned_total.saturating_add(bought);
        state.steps = state.steps.saturating_add(1);
        if PARAMS.dip_only {
            state.ref_price = dip_ref.price;
            state.ref_at = dip_ref.at;
            state.last_buy_at = dip_ref.last_buy_at;
        }

        Ok(())
    }
}

/// The token-hook slice the swap and the burn pass: the hook, the token program's signer for it,
/// then the hook's registry extras, which for the hook paired with this script is one account, its
/// state for the mint (writable). Empty for a coin without a hook.
pub fn hook_slice(custom_hook: Option<Pubkey>, mint: &Pubkey) -> Vec<AccountMeta> {
    match custom_hook {
        Some(hook) => {
            let hook_state = Pubkey::find_program_address(&[b"state", mint.as_ref()], &hook).0;
            launch_client::custom_hook_slice(
                hook,
                vec![AccountMeta {
                    pubkey: hook_state,
                    is_signer: false,
                    is_writable: true,
                }],
            )
        }
        None => Vec::new(),
    }
}

/// The token balance of the holding at `key` among `accounts`. A holding that is missing or does
/// not read is an error, not a zero: a zero before and after the swap would burn nothing silently.
fn holding_amount(accounts: &[AccountInfo], key: &Pubkey) -> Result<u64> {
    let info = accounts
        .iter()
        .find(|a| a.key == key)
        .ok_or(error!(ScriptError::BadAccount))?;
    let holding = token_client::read_holding(info).map_err(|_| error!(ScriptError::BadAccount))?;
    Ok(holding.amount)
}
