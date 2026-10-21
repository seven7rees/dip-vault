//! T19 — Dip Vault layouts against the real accounts, and the Workshop's source rules.

use anchor_lang::prelude::Pubkey;
use anchor_lang::{AccountSerialize, Space};
use bordrless_companion::state::{Companion, GameKind, NativeSol, QuoteCutClaimed, Split};
use dipvault_script::layout::{read_companion_beneficiary, read_companion_mint, read_pending_pot, read_pool_reserves};
use dipvault_script::ScriptState;

fn pubkey(n: u8) -> Pubkey {
    Pubkey::new_from_array([n; 32])
}

fn account_bytes<T: AccountSerialize>(acct: &T) -> Vec<u8> {
    let mut buf = Vec::new();
    acct.try_serialize(&mut buf).expect("try_serialize");
    buf
}

fn make_companion() -> Companion {
    Companion {
        version: 1,
        bump: 200,
        creator_bump: 201,
        mint: pubkey(7),
        beneficiary: pubkey(8),
        split: Split {
            buyback_bps: 5_000,
            holders_bps: 3_000,
            beneficiary_bps: 2_000,
        },
        bounty_bps: 50,
        max_buyback: 1_000_000_000,
        buyback_interval: 300,
        vest_secs: 86400,
        launched: true,
        launched_at: 1_760_000_000,
        dev_tokens: 1_000_000,
        dev_released: 500_000,
        pending_buyback: 111_111,
        pending_holders: 222_222,
        pending_beneficiary: 333_333,
        last_buyback_at: 1_760_000_300,
        claimed_total: 999_999,
        spent_total: 888_888,
        burned_total: 777_777,
        shared_total: 666_666,
        paid_beneficiary_total: 555_555,
        bounties_total: 444_444,
        reference_price: 12_345_678_901_234_567_890_u128,
        reference_at: 1_760_000_600,
        game_hook: Pubkey::default(),
        pot_bps: 0,
        // ← the field under test
        pending_pot: 0x_DEAD_BEEF_CAFE_1234_u64,
        round_secs: 0,
        stranded_burned_at: 0,
        game_kind: GameKind::Lottery,
        pot_locked: 0,
        mode: 2, // MODE_SCRIPT
        quote_cut_claimed: QuoteCutClaimed {
            amount: 42_000,
            stored: true,
        },
        native_sol: NativeSol {
            on: true,
            stored: true,
        },
    }
}

#[test]
fn companion_beneficiary_mint_and_pot_read_at_their_offsets() {
    let bytes = account_bytes(&make_companion());
    assert_eq!(read_companion_mint(&bytes), Some(pubkey(7)));
    assert_eq!(read_companion_beneficiary(&bytes), Some(pubkey(8)));
    assert_eq!(read_pending_pot(&bytes), Some(0x_DEAD_BEEF_CAFE_1234_u64));
}

#[test]
fn pool_lp_mint_lp_supply_and_curve_read_from_a_real_pool() {
    use bordrless_swap::state::Pool;
    for (curve, hook) in [(true, Some(bordrless_launch::ID)), (false, Some(bordrless_launch::ID)), (false, None)] {
        let p = Pool {
            version: 1,
            bump: 1,
            lp_mint_bump: 1,
            base_mint: pubkey(1),
            quote_mint: pubkey(2),
            lp_mint: pubkey(3),
            base_vault: pubkey(4),
            quote_vault: pubkey(5),
            hook_program: hook,
            hook_flags: 7,
            lp_fee_bps: 30,
            protocol_fee_bps: 0,
            base_reserve: 11,
            quote_reserve: 22,
            virtual_base: 33,
            virtual_quote: 44,
            lp_supply: 55,
            protocol_fees_quote: 66,
            curve,
            creator: pubkey(6),
            created_at: 7,
            last_swap_at: 8,
            swap_count: 9,
            base_volume: 10,
            quote_volume: 11,
            hook_signer_bump: 12,
            fee_model: 1,
            protocol_share_bps: 2_500,
            reserved: [0; 60],
        };
        let bytes = account_bytes(&p);
        let r = read_pool_reserves(&bytes).expect("reads");
        assert_eq!((r.base_mint, r.quote_mint, r.lp_mint), (pubkey(1), pubkey(2), pubkey(3)));
        assert_eq!((r.lp_fee_bps, r.base_reserve, r.quote_reserve, r.virtual_base, r.virtual_quote), (30, 11, 22, 33, 44));
        assert_eq!((r.lp_supply, r.curve), (55, curve));
        let s = bordrless_strategy::pool_reserves(&bytes).unwrap();
        assert_eq!((s.base_reserve, s.quote_reserve, s.virtual_base, s.virtual_quote), (11, 22, 33, 44));
    }
}

#[test]
fn script_state_is_320_bytes_at_its_offsets() {
    assert_eq!(8 + ScriptState::INIT_SPACE, 320, "keeper.json stateLen");
    let s = ScriptState {
        version: 1,
        mint: pubkey(0xBB),
        last_pot: 1,
        owed: 2,
        dip_pile: 3,
        holder_pile: 4,
        lp_pile: 5,
        paid_builder: 6,
        tokens_held: 7,
        steps: 8,
        ref_price: 9,
        ref_at: 10,
        last_buy_at: 11,
        round: 12,
        w_prev: 13,
        w_cur: 14,
        round_budget: 15,
        round_paid: 16,
        measured: 17,
        holders_paid: 18,
        lp_spent: 19,
        lp_minted: 20,
        lp_last_at: 21,
        reserved: [0; 95],
    };
    let d = account_bytes(&s);
    assert_eq!(d.len(), 320);
    assert_eq!(d[8], 1);
    assert_eq!(&d[9..41], pubkey(0xBB).as_ref());
    let u64_at = |at: usize| u64::from_le_bytes(d[at..at + 8].try_into().unwrap());
    let u128_at = |at: usize| u128::from_le_bytes(d[at..at + 16].try_into().unwrap());
    let u32_at = |at: usize| u32::from_le_bytes(d[at..at + 4].try_into().unwrap());
    assert_eq!([u64_at(41), u64_at(49), u64_at(57), u64_at(65), u64_at(73), u64_at(81), u64_at(89), u64_at(97)], [1, 2, 3, 4, 5, 6, 7, 8]);
    assert_eq!(u128_at(105), 9);
    assert_eq!((u64_at(121), u64_at(129)), (10, 11));
    assert_eq!(u32_at(137), 12);
    assert_eq!((u128_at(141), u128_at(157)), (13, 14));
    assert_eq!((u64_at(173), u64_at(181)), (15, 16));
    assert_eq!(u32_at(189), 17);
    assert_eq!((u64_at(193), u64_at(201), u64_at(209), u64_at(217)), (18, 19, 20, 21));
    assert_eq!(&d[225..], &[0u8; 95][..]);
    // keeper.json `view` reads these offsets live on the coin's page.
    let keeper = include_str!("../../script_variants/dipvault/keeper.json").replace([' ', '\n'], "");
    for (label, offset) in [("Dippile", 57), ("Holderpile", 65), ("Liquiditypile", 73), ("Paidtoholders", 193), ("CoinsheldforLP", 89), ("SOLintoliquidity", 201)] {
        assert!(keeper.contains(&format!("\"label\":\"{label}\",\"account\":\"script\",\"offset\":{offset},\"width\":8")), "view {label} at {offset}");
    }
}

// ─── the Workshop's source rules, as the hook and companion briefs state them ─────────────────

/// A source with its line comments and string literals blanked, roughly as the Workshop reads it.
fn blanked(src: &str) -> String {
    src.lines()
        .map(|l| {
            let code = l.split("//").next().unwrap_or("");
            let mut out = String::new();
            let mut in_str = false;
            for c in code.chars() {
                if c == '"' {
                    in_str = !in_str;
                } else if !in_str {
                    out.push(c);
                }
            }
            out
        })
        .collect::<Vec<_>>()
        .join("\n")
}

const HOOK_SOURCES: [&str; 2] = [
    include_str!("../../hook_variants/dipvault/src/lib.rs"),
    include_str!("../../hook_variants/dipvault/src/clock.rs"),
];
const SCRIPT_SOURCES: [&str; 8] = [
    include_str!("../../script_variants/dipvault/src/lib.rs"),
    include_str!("../../script_variants/dipvault/src/params.rs"),
    include_str!("../../script_variants/dipvault/src/math.rs"),
    include_str!("../../script_variants/dipvault/src/layout.rs"),
    include_str!("../../script_variants/dipvault/src/dip.rs"),
    include_str!("../../script_variants/dipvault/src/record.rs"),
    include_str!("../../script_variants/dipvault/src/holders.rs"),
    include_str!("../../script_variants/dipvault/src/liquidity.rs"),
];

#[test]
fn the_hook_calls_no_program_and_moves_no_sol_in_its_own_source() {
    let code: String = HOOK_SOURCES.iter().map(|s| blanked(s)).collect();
    for forbidden in ["invoke(", "invoke_signed", "CpiContext", "cpi::", "unsafe", "asm!", "macro_rules!", "cfg_attr", "extern", "no_mangle", "include_str!", "include_bytes!", "env!", "std::fs", "std::net", "std::process", "std::env", "std::thread", "realloc", "close =", "sub_lamports", "add_lamports", "lamports.borrow_mut"] {
        assert!(!code.contains(forbidden), "hook source contains `{forbidden}`");
    }
    assert_eq!(code.matches("declare_id!").count(), 1);
    assert_eq!(code.matches("#[program]").count(), 1);
    assert!(code.contains("pub fn prepare("));
}

#[test]
fn the_script_keeps_the_common_rules() {
    let code: String = SCRIPT_SOURCES.iter().map(|s| blanked(s)).collect();
    for forbidden in ["unsafe", "asm!", "macro_rules!", "cfg_attr", "extern", "no_mangle", "include_str!", "include_bytes!", "env!", "std::fs", "std::net", "std::process", "std::env", "std::thread"] {
        assert!(!code.contains(forbidden), "script source contains `{forbidden}`");
    }
    assert_eq!(code.matches("declare_id!").count(), 1);
    assert_eq!(code.matches("#[program]").count(), 1);
    let bytes: usize = SCRIPT_SOURCES.iter().map(|s| s.len()).sum();
    assert!(bytes <= 120_000, "{bytes} bytes");
    let hook_bytes: usize = HOOK_SOURCES.iter().map(|s| s.len()).sum();
    assert!(hook_bytes <= 120_000);
}

/// The Workshop's three forbidden words, as whole words, with the one allowed phrase removed
/// first; over every text that ships or is sent out.
#[test]
fn never_the_three_forbidden_words() {
    let texts: Vec<&str> = HOOK_SOURCES
        .iter()
        .chain(SCRIPT_SOURCES.iter())
        .copied()
        .chain([
            include_str!("../../variants/dipvault/notes.txt"),
            include_str!("../../script_variants/dipvault/keeper.json"),
            include_str!("../../DIP-VAULT-FORMAT-NOTE.txt"),
        ])
        .collect();
    for src in texts {
        let lower = src.to_lowercase().replace("not audited", "");
        let words: Vec<String> = lower
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .map(str::to_string)
            .collect();
        for forbidden in ["audited", "safe", "guaranteed"] {
            assert!(!words.iter().any(|w| w == forbidden), "found `{forbidden}`");
        }
    }
}
