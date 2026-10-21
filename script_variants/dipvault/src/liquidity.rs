//! Locked liquidity: a tranche of the liquidity pile buys the coin on its own pool and pairs the
//! coins bought with SOL as liquidity on that pool. The LP it mints lands in the launch's own LP
//! holding (`Launch.lp_holding`), which no one can spend, so the liquidity is locked for good.
//! Only after graduation: on the bonding curve the DEX refuses to add liquidity.
//!
//! The DEX takes an add's quote as bridged SOL, so the pairing half is wrapped first through the
//! bridge, exactly the amount the add takes. The builders here are the bridge's `wrap_sol` and
//! the DEX's `add_liquidity`, account for account and byte for byte (the script template has
//! neither crate); the harness pins them to the programs' own clients.

use anchor_lang::prelude::*;
use anchor_lang::solana_program::instruction::{AccountMeta, Instruction};
use bordrless_launch::constants::{DEX_CONFIG, DEX_EVENT_AUTHORITY, DEX_HOOK_AUTHORITY};
use bordrless_token::client as token_client;

use crate::math::ceil_ratio;
use crate::params::LP_BUY_SHARE_BPS;
use crate::DEX_ID;

/// The Bordrless bridge: wraps SOL into bridged SOL.
pub const BRIDGE_ID: Pubkey = Pubkey::from_str_const("CtLkuFVitoXHTa86Hfp8KmfSDfqJaMYFWr6EGmQVsKb7");
/// The native SOL mint the bridge wraps.
pub const NATIVE_MINT: Pubkey = Pubkey::from_str_const("So11111111111111111111111111111111111111112");
/// Anchor's discriminator of the bridge's `wrap_sol`: `sha256("global:wrap_sol")[..8]`.
pub const WRAP_SOL_DISCRIMINATOR: [u8; 8] = [47, 62, 155, 172, 131, 205, 37, 201];
/// Anchor's discriminator of the DEX's `add_liquidity`: `sha256("global:add_liquidity")[..8]`.
pub const ADD_LIQUIDITY_DISCRIMINATOR: [u8; 8] = [181, 157, 89, 67, 143, 182, 52, 72];

/// How a tranche of `tranche` lamports is spent: `buy` lamports buy the coin, the rest pairs.
pub fn buy_part(tranche: u64) -> u64 {
    u64::try_from(u128::from(tranche) * u128::from(LP_BUY_SHARE_BPS) / 10_000).unwrap_or(0)
}

/// The add the DEX will make for `bought` coins at the pool's ratio after the buy, with at most
/// `quote_budget` lamports of SOL to pair: `(base_desired, quote_desired)`, exactly what the DEX
/// pulls, so nothing wrapped is left over. Pairs every coin when the budget allows, else as many
/// as the budget pairs. `None` when nothing can be paired.
pub fn pairing(
    bought: u64,
    base_reserve: u64,
    quote_reserve: u64,
    quote_budget: u64,
) -> Option<(u64, u64)> {
    if bought == 0 || base_reserve == 0 || quote_reserve == 0 || quote_budget == 0 {
        return None;
    }
    let q = ceil_ratio(bought, quote_reserve, base_reserve)?;
    if q <= quote_budget {
        return Some((bought, q));
    }
    let b = ceil_ratio(quote_budget, base_reserve, quote_reserve)?;
    if b == 0 || b > bought {
        return None;
    }
    let q2 = ceil_ratio(b, quote_reserve, base_reserve)?;
    Some((b, q2.min(quote_budget)))
}

/// The pool's LP mint: `["lp", pool]` under the DEX.
pub fn lp_mint_address(pool: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[b"lp", pool.as_ref()], &DEX_ID).0
}

fn bridge_pda(seeds: &[&[u8]]) -> Pubkey {
    Pubkey::find_program_address(seeds, &BRIDGE_ID).0
}

/// The bridge's `wrap_sol(lamports)` for `user`: `user`'s lamports into its bridged-SOL holding.
pub fn wrap_sol(user: Pubkey, lamports: u64) -> Instruction {
    let wrapped = bridge_pda(&[b"wrapped", NATIVE_MINT.as_ref()]);
    let accounts = vec![
        AccountMeta::new(user, true),
        AccountMeta::new_readonly(bridge_pda(&[b"config"]), false),
        AccountMeta::new(bridge_pda(&[b"wrapper", NATIVE_MINT.as_ref()]), false),
        AccountMeta::new(bridge_pda(&[b"sol-vault"]), false),
        AccountMeta::new(wrapped, false),
        AccountMeta::new(token_client::holding_address(&wrapped, &user), false),
        AccountMeta::new_readonly(bordrless_token::ID, false),
        AccountMeta::new_readonly(token_client::event_authority(), false),
        AccountMeta::new_readonly(anchor_lang::system_program::ID, false),
        AccountMeta::new_readonly(bridge_pda(&[b"__event_authority"]), false),
        AccountMeta::new_readonly(BRIDGE_ID, false),
    ];
    let mut data = WRAP_SOL_DISCRIMINATOR.to_vec();
    data.extend_from_slice(&lamports.to_le_bytes());
    Instruction {
        program_id: BRIDGE_ID,
        accounts,
        data,
    }
}

/// The keys of a liquidity add on a launch pool.
#[derive(Clone, Copy, Debug)]
pub struct AddKeys {
    /// The companion's creator address: the provider.
    pub provider: Pubkey,
    pub pool: Pubkey,
    pub mint: Pubkey,
    pub quote_mint: Pubkey,
    /// `Launch.lp_holding`: the launch's own LP holding, where the LP minted lands.
    pub lp_holding: Pubkey,
}

/// The DEX's `add_liquidity` on the launch's pool by `provider`, the LP minted into the launch's
/// own LP holding (account 10: the DEX's client would name the provider's); `base_hook_slice` is
/// the coin's hook slice (its transfer into the vault runs the hook).
pub fn add_liquidity(
    k: &AddKeys,
    base_desired: u64,
    quote_desired: u64,
    min_lp: u64,
    base_hook_slice: Vec<AccountMeta>,
) -> Instruction {
    let mut accounts = vec![
        AccountMeta::new(k.provider, true),
        AccountMeta::new_readonly(DEX_CONFIG, false),
        AccountMeta::new(k.pool, false),
        AccountMeta::new_readonly(k.mint, false),
        AccountMeta::new_readonly(k.quote_mint, false),
        AccountMeta::new(lp_mint_address(&k.pool), false),
        AccountMeta::new(token_client::holding_address(&k.mint, &k.pool), false),
        AccountMeta::new(token_client::holding_address(&k.quote_mint, &k.pool), false),
        AccountMeta::new(token_client::holding_address(&k.mint, &k.provider), false),
        AccountMeta::new(token_client::holding_address(&k.quote_mint, &k.provider), false),
        AccountMeta::new(k.lp_holding, false),
        AccountMeta::new_readonly(bordrless_launch::ID, false),
        AccountMeta::new_readonly(DEX_HOOK_AUTHORITY, false),
        AccountMeta::new_readonly(bordrless_token::ID, false),
        AccountMeta::new_readonly(token_client::event_authority(), false),
        AccountMeta::new_readonly(DEX_EVENT_AUTHORITY, false),
        AccountMeta::new_readonly(DEX_ID, false),
    ];
    let slice_len = base_hook_slice.len() as u8;
    accounts.extend(base_hook_slice);
    // AddLiquidityArgs, Borsh: base_desired u64, quote_desired u64, min_lp u64,
    // base_hook_accounts u8, quote_hook_accounts u8, hook_data Vec<u8> (empty).
    let mut data = ADD_LIQUIDITY_DISCRIMINATOR.to_vec();
    data.extend_from_slice(&base_desired.to_le_bytes());
    data.extend_from_slice(&quote_desired.to_le_bytes());
    data.extend_from_slice(&min_lp.to_le_bytes());
    data.push(slice_len);
    data.push(0);
    data.extend_from_slice(&0u32.to_le_bytes());
    Instruction {
        program_id: DEX_ID,
        accounts,
        data,
    }
}
