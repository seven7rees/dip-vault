//! SOL cut on sells, and a count of burned coins.
//!
//! # What it does
//! * Sells pay a cut of the SOL the seller receives. The cut grows with the size of the sell next to
//!   the pool: 0.78 times the sell's share of the pool's depth, capped at 7.8%. Buys pay no cut.
//!   The launchpad adds 25% of the cut on top, so a seller pays up to about 9.75% of the SOL out.
//! * After every burn it adds the burned amount to a counter in the hook's state.
//! * It never touches buys, sells or sends, so it cannot refuse an ordinary trade. The only token
//!   callback it runs is `after_burn` (flag AFTER_BURN = 32).
//!
//! # hook.json
//! `{ "version": 1, "quoteCut": { "maxBuyBps": 1, "maxSellBps": 780 } }`. The launchpad needs a buy cap
//! of at least 1 because it asks on every buy; this hook always answers 0 on buys.
//!
//! # State layout (PDA ["state", mint] under this program; 50 bytes)
//! offset  0 : discriminator [u8; 8]   (Anchor)
//! offset  8 : version       u8        (always 1)
//! offset  9 : bump          u8        (PDA bump for ["state", mint])
//! offset 10 : mint          [u8; 32]  (the mint this state was prepared for)
//! offset 42 : burned        u64 LE    (total coins burned on this mint since launch)

#![allow(unexpected_cfgs)]

use anchor_lang::prelude::*;
use anchor_lang::solana_program::program::set_return_data;
use bordrless_hook::{
    hook_accounts_address, hook_signer, write_registry, AccountSource, ExtraAccount,
    HookAccountList, Phase, PoolHookArgs, Seed, TokenHookArgs,
};

// Placeholder program id: Bordrless writes the real one when it builds the source.
declare_id!("11111111111111111111111111111111");

// ─── Program ids (the same on mainnet, devnet and localnet) ──────────────────

/// Bordrless token program: the only valid caller of the token hook callbacks.
pub const TOKEN_PROGRAM: Pubkey =
    Pubkey::from_str_const("2XoEWp8cF3kRXg74eVwPAyTFhVCAztn3V88komxAvr22");

/// Bordrless launchpad: the only valid caller of `quote_cut`.
pub const LAUNCHPAD: Pubkey =
    Pubkey::from_str_const("1jcBymHxBjniZDhNPy51Vgm5Nz7pLUdxa9UBHc4TavC");

// ─── PDA seeds ────────────────────────────────────────────────────────────────

/// State PDA seed: `["state", mint]` under this program.
pub const STATE_SEED: &[u8] = b"state";

/// Seed the launchpad uses to derive its signer for this hook: `["quote-cut", hook]`.
pub const QUOTE_CUT_SEED: &[u8] = b"quote-cut";

// ─── Cut-curve constants ─────────────────────────────────────────────────────

/// Slope used in the sell-cut formula (bps-of-depth per whole depth).
///
/// `cut_bps = amount_out × CUT_SLOPE_BPS / depth`  (floor, u128 intermediates)
///
/// Interpretation: a seller who removes 1 % of the pool depth pays 78 bps (0.78 %);
/// one who removes 10 % hits the cap.  Formula: 7800 bps × share = 78 × share-in-%.
pub const CUT_SLOPE_BPS: u128 = 7_800;

/// Hard cap on the sell-side SOL cut: 780 bps = 7.80 %. Must match hook.json `maxSellBps`.
pub const CUT_CAP_BPS: u16 = 780;

/// SOL cut on buys: 0 (the launchpad always asks; we always answer 0).
/// `hook.json.maxBuyBps` = 1 so the launchpad is permitted to ask but we always return 0.
pub const BUY_CUT_BPS: u16 = 0;

/// Swap direction value meaning quote → base (a buy).
const DIRECTION_BUY: u8 = 1;

// ─── Registry ─────────────────────────────────────────────────────────────────

/// Extra accounts appended to every token-hook callback invocation.
///
/// Exactly one extra: the hook state, writable.
/// Seed Account(1) resolves to the mint (index 1 in the five-account token-hook prefix:
/// hook_signer(0), mint(1), source(2), destination(3), authority(4)).
pub fn extra_accounts() -> HookAccountList {
    HookAccountList::new(vec![ExtraAccount {
        writable: true,
        source: AccountSource::Pda {
            program: crate::ID,
            seeds: vec![Seed::Literal(STATE_SEED.to_vec()), Seed::Account(1)],
        },
    }])
}

// ─── Pure helper ──────────────────────────────────────────────────────────────

/// SOL cut on a sell in basis points, given post-swap pool state.
///
/// `depth = quote_reserve + virtual_quote + amount_out`
/// (the pre-sale total SOL in the pool; `amount_out` is net of the DEX's sell protocol fee).
///
/// Returns 0 when `depth == 0` or `amount_out == 0`.
/// Otherwise: `min(amount_out × CUT_SLOPE_BPS / depth, CUT_CAP_BPS)`.
///
/// All arithmetic uses u128 intermediates; cannot overflow or panic on any u64 input.
pub fn sell_cut_bps(amount_out: u64, quote_reserve: u64, virtual_quote: u64) -> u16 {
    if amount_out == 0 {
        return 0;
    }
    // depth: pre-sale pool SOL depth (u128 to handle three u64 addends without overflow)
    let depth: u128 = (quote_reserve as u128)
        .saturating_add(virtual_quote as u128)
        .saturating_add(amount_out as u128);
    if depth == 0 {
        return 0;
    }
    // raw is in bps; saturating_mul is safe because both operands are at most u64::MAX < u128::MAX/2
    let raw: u128 = (amount_out as u128).saturating_mul(CUT_SLOPE_BPS) / depth;
    if raw >= CUT_CAP_BPS as u128 {
        CUT_CAP_BPS
    } else {
        raw as u16
    }
}

// ─── Program ──────────────────────────────────────────────────────────────────

#[program]
pub mod studio_hook {
    use super::*;

    /// Standard `prepare`: creates the hook state for `mint` and writes the extra-accounts
    /// registry. Anyone may send it, once per mint (a second call fails because the state exists).
    /// The launch sends it for every new mint.
    ///
    /// State written: version = 1, bump, mint = mint key, burned = 0.
    /// Registry written: one extra account (the writable state PDA).
    pub fn prepare(ctx: Context<Prepare>) -> Result<()> {
        let mint = ctx.accounts.mint.key();

        // Initialise the state.
        let state = &mut ctx.accounts.state;
        state.version = 1;
        state.bump = ctx.bumps.state;
        state.mint = mint;
        state.burned = 0;

        // Derive and verify the registry address, then write it.
        let (registry_addr, registry_bump) = hook_accounts_address(&crate::ID, &mint);
        require_keys_eq!(
            ctx.accounts.registry.key(),
            registry_addr,
            HookError::WrongAccount
        );
        write_registry(
            &ctx.accounts.payer.to_account_info(),
            &ctx.accounts.registry.to_account_info(),
            &ctx.accounts.system_program.to_account_info(),
            &crate::ID,
            &mint,
            registry_bump,
            &extra_accounts(),
        )
    }

    /// Launchpad's SOL-cut callback: invoked on every buy (Before phase) and every sell
    /// (After phase) of any coin launched with this hook.
    ///
    /// Answers (via `set_return_data`):
    ///   • 0 bps  on buys  (direction == 1)           — buys are always free
    ///   • 0 bps  on non-After-phase sells             — pre-swap reserves are not yet settled
    ///   • sell_cut_bps on After-phase sells            — post-swap depth is the ground truth
    ///
    /// This function:
    ///   - Verifies that the signer is the launchpad's `["quote-cut", hook]` PDA.
    ///   - Reads only; never writes any account.
    ///   - Never fails for any valid launchpad call (valid signer, any reserve values).
    ///   - Never panics on any u64 input (all arithmetic is saturating / u128).
    pub fn quote_cut(ctx: Context<QuoteCut>, args: PoolHookArgs) -> Result<()> {
        // Trust only the launchpad's dedicated signer PDA for this hook program.
        // PDA: ["quote-cut", hook_program_id] under the launchpad.
        let (expected_signer, _) =
            Pubkey::find_program_address(&[QUOTE_CUT_SEED, crate::ID.as_ref()], &LAUNCHPAD);
        require!(
            ctx.accounts.signer.is_signer
                && ctx.accounts.signer.key() == expected_signer,
            HookError::NotLaunchpad
        );

        // Buys (direction 1): always 0.
        // Non-After sells: reserves are pre-swap; answer 0 until the After phase.
        let bps: u16 = if args.direction == DIRECTION_BUY || args.phase != Phase::After {
            BUY_CUT_BPS // 0
        } else {
            // After-phase sell: the reserves are already updated, and amount_out is net of the DEX's
            // sell protocol fee. Pre-sale depth = quote_reserve + virtual_quote + amount_out.
            sell_cut_bps(args.amount_out, args.quote_reserve, args.virtual_quote)
        };

        set_return_data(&bps.to_le_bytes());
        Ok(())
    }

    /// After every burn on this mint: accumulate the burned amount in the state.
    ///
    /// This is the only active token-hook callback (AFTER_BURN = 32).
    /// It never refuses any operation and never affects buys, sells or sends.
    pub fn after_burn(ctx: Context<Callback>, args: TokenHookArgs) -> Result<()> {
        check_call(&ctx, &args)?;
        // Saturating add: burned counter never panics on any valid token supply.
        ctx.accounts.state.burned =
            ctx.accounts.state.burned.saturating_add(args.amount);
        Ok(())
    }
}

// ─── Callback guard ───────────────────────────────────────────────────────────

/// Verifies that a token-hook callback is genuine.
///
/// Two checks (both must pass, or the callback is refused):
///   1. The hook signer is the token program's PDA `["hook-authority", hook]`  and is signing.
///   2. `args.mint` equals the mint this state was prepared for.
fn check_call(ctx: &Context<Callback>, args: &TokenHookArgs) -> Result<()> {
    let signer = &ctx.accounts.hook_signer;
    require!(signer.is_signer, HookError::BadHookSigner);
    require_keys_eq!(
        signer.key(),
        hook_signer(&TOKEN_PROGRAM, &crate::ID).0,
        HookError::BadHookSigner
    );
    require_keys_eq!(args.mint, ctx.accounts.state.mint, HookError::WrongAccount);
    Ok(())
}

// ─── State ────────────────────────────────────────────────────────────────────

/// Hook state for one mint.  PDA: `["state", mint]` under this program.
///
/// Layout (+ 8-byte Anchor discriminator = **50 bytes** total; see module-level doc):
///   offset  0: discriminator [u8; 8]
///   offset  8: version       u8         always 1
///   offset  9: bump          u8         PDA bump
///   offset 10: mint          [u8; 32]
///   offset 42: burned        u64 LE     cumulative tokens burned by the companion script
#[account]
#[derive(InitSpace)]
pub struct HookState {
    /// Layout version; always 1.
    pub version: u8,
    /// PDA bump for `["state", mint]` under this program.
    pub bump: u8,
    /// The mint this hook state was prepared for.
    pub mint: Pubkey,
    /// Cumulative tokens burned by the companion script (via `burn_with`), saturating.
    pub burned: u64,
}

// ─── Accounts structs ─────────────────────────────────────────────────────────

/// Accounts for `prepare`.
#[derive(Accounts)]
pub struct Prepare<'info> {
    /// Pays the rent for the state and registry.
    #[account(mut)]
    pub payer: Signer<'info>,
    /// CHECK: the mint the hook is being prepared for; need not exist yet.
    pub mint: UncheckedAccount<'info>,
    /// Hook state PDA `["state", mint]`: created here, owned by this program.
    #[account(
        init,
        payer = payer,
        space = 8 + HookState::INIT_SPACE,
        seeds = [STATE_SEED, mint.key().as_ref()],
        bump,
    )]
    pub state: Box<Account<'info, HookState>>,
    /// CHECK: registry PDA `["bordrless-hook-accounts", mint]`; address verified in the handler.
    #[account(mut)]
    pub registry: UncheckedAccount<'info>,
    pub system_program: Program<'info, System>,
}

/// Accounts for the launchpad's `quote_cut` callback.
///
/// Account order is the launchpad's:
///   [0] signer        — launchpad's `["quote-cut", hook]` PDA (signer)
///   [1] launch        — launch account (read-only)
///   [2] pool          — pool account (read-only)
///   [3] base_mint     — token mint (read-only)
///   [4] quote_mint    — SOL-side mint (read-only)
///   [5] actor         — the trader (read-only)
///   [6] actor_holding — trader's token holding (read-only)
///   [7] state         — hook state PDA (passed writable by the launchpad; not used)
#[derive(Accounts)]
pub struct QuoteCut<'info> {
    /// CHECK: launchpad's signer PDA; verified in the handler.
    pub signer: UncheckedAccount<'info>,
    /// CHECK: the launch account.
    pub launch: UncheckedAccount<'info>,
    /// CHECK: the pool.
    pub pool: UncheckedAccount<'info>,
    /// CHECK: the base (token) mint.
    pub base_mint: UncheckedAccount<'info>,
    /// CHECK: the quote (SOL-side) mint.
    pub quote_mint: UncheckedAccount<'info>,
    /// CHECK: the trader.
    pub actor: UncheckedAccount<'info>,
    /// CHECK: the trader's token holding.
    pub actor_holding: UncheckedAccount<'info>,
    /// CHECK: this hook's state for the mint. The launchpad passes it writable and checks it; the cut
    /// never reads or writes it, so the call cannot fail for lack of it.
    pub state: UncheckedAccount<'info>,
}

/// Accounts for every token-hook callback.
///
/// Five fixed prefix accounts (token-hook protocol), then extras from the registry:
///   [0] hook_signer — token program's `["hook-authority", hook]` PDA (signer)
///   [1] mint        — the token mint
///   [2] source      — source holding (the mint for a mint op)
///   [3] destination — destination holding (the mint for a burn)
///   [4] authority   — who signed the operation
///   [5] state       — hook state PDA `["state", mint]` (writable; from registry)
#[derive(Accounts)]
pub struct Callback<'info> {
    /// CHECK: token program's signer for this hook; verified in `check_call`.
    pub hook_signer: UncheckedAccount<'info>,
    /// CHECK: the token mint.
    pub mint: UncheckedAccount<'info>,
    /// CHECK: source holding (or the mint for a mint op).
    pub source: UncheckedAccount<'info>,
    /// CHECK: destination holding (or the mint for a burn).
    pub destination: UncheckedAccount<'info>,
    /// CHECK: who signed the operation.
    pub authority: UncheckedAccount<'info>,
    /// Hook state `["state", mint]`.
    #[account(mut, seeds = [STATE_SEED, mint.key().as_ref()], bump = state.bump)]
    pub state: Box<Account<'info, HookState>>,
}

// ─── Errors ───────────────────────────────────────────────────────────────────

#[error_code]
pub enum HookError {
    #[msg("account key does not match what this hook's mint expects")]
    WrongAccount,
    #[msg("caller is not the token program's signer for this hook")]
    BadHookSigner,
    #[msg("caller is not the launchpad's signer for this hook")]
    NotLaunchpad,
}
