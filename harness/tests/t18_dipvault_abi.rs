//! T18 — Dip Vault ABI pins: every address, discriminator, account list and encoding the pair
//! hard-codes or builds by hand, against the programs' own crates at the pinned revision.

use anchor_lang::prelude::Pubkey;
use anchor_lang::solana_program::instruction::AccountMeta;
use anchor_lang::{AccountSerialize, AnchorSerialize};

fn borsh<T: AnchorSerialize>(x: &T) -> Vec<u8> {
    let mut v = Vec::new();
    x.serialize(&mut v).unwrap();
    v
}

use bordrless_companion::constants::{
    COMPANION_INIT_DISCRIMINATOR, COMPANION_STEP_DISCRIMINATOR, PAYOUT_AUTHORITY,
    RECORD_PAYOUT_DISCRIMINATOR, SCRIPT_BOUNTY,
};
use bordrless_companion::state::{PayoutAnswer, PayoutEntry, WalletPayoutAnswer, WalletPayoutEntry};

fn key(n: u8) -> Pubkey {
    Pubkey::new_from_array([n; 32])
}

// ─── the hook ────────────────────────────────────────────────────────────────

#[test]
fn the_hooks_program_ids_and_seeds_are_the_protocols() {
    assert_eq!(dipvault_hook::SWAP_ID, bordrless_swap::ID);
    assert_eq!(dipvault_hook::BRIDGED_SOL_MINT, bordrless_swap::constants::BRIDGED_SOL_MINT);
    assert_eq!(dipvault_hook::PAYOUT_AUTHORITY, PAYOUT_AUTHORITY);
    assert_eq!(bordrless_game::LAUNCH_PROGRAM_ID, bordrless_launch::ID);
    assert_eq!(bordrless_game::TOKEN_PROGRAM_ID, bordrless_token::ID);
    assert_eq!(bordrless_game::COMPANION_PROGRAM_ID, bordrless_companion::ID);
    let mint = key(3);
    assert_eq!(
        dipvault_hook::pool_address(&mint),
        bordrless_launch::client::pool_address(&mint, &bordrless_swap::constants::BRIDGED_SOL_MINT, 30)
    );
    assert_eq!(dipvault_hook::lp_mint_address(&key(4)), bordrless_swap::client::lp_mint_address(&key(4)));
    assert_eq!(
        dipvault_hook::holding_address(&key(5), &key(6)),
        bordrless_token::client::holding_address(&key(5), &key(6))
    );
    assert_eq!(dipvault_hook::own_hook_authority(), bordrless_hook::hook_authority(&dipvault_hook::ID).0);
}

#[test]
fn the_registry_lists_state_lp_mint_and_the_launches_lp_holding() {
    use bordrless_hook::{AccountSource, Seed};
    let mint = key(9);
    let list = dipvault_hook::extra_accounts(&mint);
    assert_eq!(list.accounts.len(), 3, "at most 4 extras; three here");
    assert!(list.accounts.iter().all(|a| a.writable));
    assert_eq!(
        list.accounts[0].source,
        AccountSource::Pda { program: dipvault_hook::ID, seeds: vec![Seed::Literal(b"state".to_vec()), Seed::Account(1)] }
    );
    let pool = dipvault_hook::pool_address(&mint);
    let lp_mint = bordrless_swap::client::lp_mint_address(&pool);
    assert_eq!(list.accounts[1].source, AccountSource::Key(lp_mint));
    assert_eq!(
        list.accounts[2].source,
        AccountSource::Key(bordrless_token::client::holding_address(&lp_mint, &bordrless_launch::client::launch_address(&mint)))
    );
    // The script's hook slice passes the same three, in the same order, all writable.
    let slice = dipvault_script::hook_slice(Some(dipvault_hook::ID), &mint, &pool);
    assert_eq!(slice.len(), 5, "hook, the token program's signer for it, three extras");
    assert_eq!(slice[0].pubkey, dipvault_hook::ID);
    assert_eq!(slice[1].pubkey, bordrless_token::client::hook_signer(&dipvault_hook::ID));
    let state = Pubkey::find_program_address(&[b"state", mint.as_ref()], &dipvault_hook::ID).0;
    assert_eq!((slice[2].pubkey, slice[2].is_writable), (state, true));
    assert_eq!((slice[3].pubkey, slice[3].is_writable), (lp_mint, true));
    assert_eq!(slice[4].pubkey, bordrless_token::client::holding_address(&lp_mint, &bordrless_launch::client::launch_address(&mint)));
    assert!(dipvault_script::hook_slice(None, &mint, &pool).is_empty());
}

#[test]
fn record_payout_is_called_as_the_companion_builds_it() {
    use anchor_lang::{Discriminator, InstructionData, ToAccountMetas};
    let (mint, owner, hook) = (key(1), key(2), dipvault_hook::ID);
    let holding = bordrless_token::client::holding_address(&mint, &owner);
    let reference = bordrless_companion::client::record_payout(hook, mint, holding, 77, 9);
    assert_eq!(&reference.data[..8], &RECORD_PAYOUT_DISCRIMINATOR[..]);
    assert_eq!(dipvault_hook::instruction::RecordPayout::DISCRIMINATOR, &RECORD_PAYOUT_DISCRIMINATOR[..]);
    assert_eq!(dipvault_hook::instruction::RecordPayout { lamports: 77, tag: 9 }.data(), reference.data);
    let ours = dipvault_hook::accounts::RecordPayout {
        payout_authority: PAYOUT_AUTHORITY,
        mint,
        holding,
        state: Pubkey::find_program_address(&[b"state", mint.as_ref()], &hook).0,
        hook_authority: dipvault_hook::own_hook_authority(),
        token_program: bordrless_token::ID,
        token_event_authority: bordrless_token::client::event_authority(),
    }
    .to_account_metas(None);
    let ours: Vec<(Pubkey, bool)> = ours.iter().map(|m| (m.pubkey, m.is_writable)).collect();
    let theirs: Vec<(Pubkey, bool)> = reference.accounts.iter().map(|m| (m.pubkey, m.is_writable)).collect();
    assert_eq!(ours, theirs);
    assert_eq!(bordrless_game::TOKEN_EVENT_AUTHORITY, bordrless_token::client::event_authority());
}

#[test]
fn the_callbacks_and_quote_cut_have_the_protocols_discriminators() {
    use anchor_lang::Discriminator;
    use bordrless_hook::discriminators as d;
    assert_eq!(dipvault_hook::instruction::BeforeTransfer::DISCRIMINATOR, &d::BEFORE_TRANSFER[..]);
    assert_eq!(dipvault_hook::instruction::BeforeBurn::DISCRIMINATOR, &d::BEFORE_BURN[..]);
    assert_eq!(dipvault_hook::instruction::QuoteCut::DISCRIMINATOR, my_hook::instruction::QuoteCut::DISCRIMINATOR, "same name as the MOON/WICK hook's");
}

#[test]
fn the_hook_reads_a_real_holdings_hook_data() {
    use bordrless_token::state::Holding;
    for delegate in [None, Some(key(8))] {
        let h = Holding {
            version: 1,
            bump: 250,
            mint: key(1),
            owner: key(2),
            amount: 123,
            delegate,
            delegated_amount: 0,
            frozen: false,
            hook_data: [0xAB; 64],
            reserved: [0; 16],
        };
        let mut data = Vec::new();
        h.try_serialize(&mut data).unwrap();
        let v = bordrless_game::parse_holding(&data).expect("parses");
        assert_eq!((v.mint, v.owner, v.amount, v.hook_data), (key(1), key(2), 123, [0xAB; 64]));
    }
}

// ─── the script ──────────────────────────────────────────────────────────────

#[test]
fn the_scripts_program_ids_and_discriminators() {
    use anchor_lang::Discriminator;
    assert_eq!(dipvault_script::COMPANION_ID, bordrless_companion::ID);
    assert_eq!(dipvault_script::DEX_ID, bordrless_swap::ID);
    assert_eq!(dipvault_script::liquidity::BRIDGE_ID, bordrless_bridge::ID);
    assert_eq!(dipvault_script::instruction::CompanionInit::DISCRIMINATOR, &COMPANION_INIT_DISCRIMINATOR[..]);
    assert_eq!(dipvault_script::instruction::CompanionStep::DISCRIMINATOR, &COMPANION_STEP_DISCRIMINATOR[..]);
    assert_eq!(dipvault_script::math::COMPANION_SCRIPT_BOUNTY, SCRIPT_BOUNTY);
    assert_eq!(dipvault_script::math::COMPANION_SCRIPT_BOUNTY, 20_000, "0.00002 SOL a step");
    assert_eq!(dipvault_script::params::PAYOUT_MAX_ENTRIES, 8, "the companion pays at most 10 holdings a step");
}

#[test]
fn wrap_sol_is_the_bridges_own() {
    let user = key(11);
    let ours = dipvault_script::liquidity::wrap_sol(user, 123_456);
    let theirs = bordrless_bridge::client::wrap_sol(user, 123_456);
    assert_eq!(ours.program_id, theirs.program_id);
    assert_eq!(ours.data, theirs.data);
    let metas = |v: &[AccountMeta]| v.iter().map(|m| (m.pubkey, m.is_signer, m.is_writable)).collect::<Vec<_>>();
    assert_eq!(metas(&ours.accounts), metas(&theirs.accounts));
}

#[test]
fn add_liquidity_is_the_dexs_own_but_for_the_lp_holding() {
    use bordrless_swap::client::{add_liquidity, LiquidityKeys};
    use dipvault_script::liquidity::{add_liquidity as ours_fn, AddKeys};
    let (provider, pool, mint, quote) = (key(21), key(22), key(23), bordrless_swap::constants::BRIDGED_SOL_MINT);
    let lp_holding = key(24);
    let slice = vec![AccountMeta::new(key(31), false), AccountMeta::new_readonly(key(32), false)];
    let ours = ours_fn(&AddKeys { provider, pool, mint, quote_mint: quote, lp_holding }, 1_000, 2_000, 3, slice.clone());
    let theirs = add_liquidity(
        &LiquidityKeys { provider, pool, base_mint: mint, quote_mint: quote, hook_program: Some(bordrless_launch::ID) },
        bordrless_swap::AddLiquidityArgs {
            base_desired: 1_000,
            quote_desired: 2_000,
            min_lp: 3,
            base_hook_accounts: 2,
            quote_hook_accounts: 0,
            hook_data: vec![],
        },
        slice,
    );
    assert_eq!(ours.program_id, theirs.program_id);
    assert_eq!(ours.data, theirs.data);
    assert_eq!(ours.accounts.len(), theirs.accounts.len());
    for (i, (a, b)) in ours.accounts.iter().zip(theirs.accounts.iter()).enumerate() {
        assert_eq!((a.is_signer, a.is_writable), (b.is_signer, b.is_writable), "account {i}");
        if i == 10 {
            assert_eq!(a.pubkey, lp_holding, "the LP goes to the launch's holding");
            assert_eq!(b.pubkey, bordrless_token::client::holding_address(&bordrless_swap::client::lp_mint_address(&pool), &provider));
        } else {
            assert_eq!(a.pubkey, b.pubkey, "account {i}");
        }
    }
}

#[test]
fn the_payout_answers_are_the_companions_borsh() {
    let ours = dipvault_script::math::encode_payout_answer(&[(3, 1_000, 7), (0, 0, 7)]);
    let theirs = borsh(&PayoutAnswer {
        version: 1,
        entries: vec![PayoutEntry { account: 3, lamports: 1_000, tag: 7 }, PayoutEntry { account: 0, lamports: 0, tag: 7 }],
    });
    assert_eq!(ours, theirs);
    assert_eq!(ours.len(), 1 + 4 + 13 * 2);
    let w = dipvault_script::math::encode_wallet_answer(&key(5), 9, 2);
    let theirs = borsh(&WalletPayoutAnswer { version: 2, entries: vec![WalletPayoutEntry { to: key(5), lamports: 9, tag: 2 }] });
    assert_eq!(&w[..], &theirs[..]);
}

#[test]
fn builder_wallet_is_the_wallet_file_and_the_keeper_names_it() {
    use std::str::FromStr;
    let file = include_str!("../../WALLET.txt").trim();
    assert_eq!(dipvault_script::params::BUILDER_WALLET, Pubkey::from_str(file).unwrap());
    assert!(dipvault_script::params::BUILDER_WALLET.is_on_curve());
    let keeper = include_str!("../../script_variants/dipvault/keeper.json");
    assert!(keeper.contains(file), "keeper.json wallets must name the builder wallet");
    let flat = keeper.replace([' ', '\n'], "");
    assert!(flat.contains("\"payouts\":true"), "the companion's payout accounts come first");
    assert!(flat.contains("\"mintWritable\":false"), "nothing is burned: the mint need not be writable");
    assert!(flat.contains("\"stepSecs\":300"));
    assert!(flat.contains("\"stateLen\":320"));
    assert!(flat.contains("\"offset\":1,\"width\":4,\"compare\":\"ne\",\"against\":{\"epoch\":{\"secs\":604800"), "the round field, not this week's epoch");
    assert_eq!(dipvault_script::params::DIP.step_secs, 300, "DIP.step_secs equals keeper stepSecs");
    assert_eq!(dipvault_script::params::ROUND_SECS, 604_800, "ROUND_SECS equals the keeper's epoch");
}
