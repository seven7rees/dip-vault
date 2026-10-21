//! T8 — Variant diff check (SPEC §4).
//!
//! Verifies that MOON, WICK, and FUEL differ exactly where they should:
//!   D1: MOON  keeper.payoutTerms.stepBps == 2000; payout_bps == 2000; needs_hook == true;  pace_steps == 480
//!   D2: WICK  keeper.payoutTerms.stepBps == 1000; payout_bps == 1000; needs_hook == true;  pace_steps == 240
//!   D3: FUEL  keeper.payoutTerms.stepBps == 1000; payout_bps == 1000; needs_hook == false; pace_steps == 240
//!   D4: All three keeper.json share the same stateLen (128) and version (1).
//!   D7: only FUEL is dip-only; MOON and WICK buy at their pace every step.
//!   D8: params.rs `DIP.step_secs` equals keeper.json `stepSecs`.
//!   D5: MOON payout_bps vs WICK/FUEL payout_bps: exactly 2× difference.
//!   D6: keeper.payoutTerms.stepBps must match params.rs payout_bps exactly.
//!
//! Files read at compile time via include_str! to catch edits that break the contract.

// ─── Embed the keeper.json files ─────────────────────────────────────────────

const MOON_KEEPER: &str = include_str!("../../variants/moon/keeper.json");
const WICK_KEEPER: &str = include_str!("../../variants/wick/keeper.json");
const FUEL_KEEPER: &str = include_str!("../../variants/fuel/keeper.json");

// ─── Embed the params.rs source for structural checks ────────────────────────

const MOON_PARAMS_SRC: &str = include_str!("../../variants/moon/params.rs");
const WICK_PARAMS_SRC: &str = include_str!("../../variants/wick/params.rs");
const FUEL_PARAMS_SRC: &str = include_str!("../../variants/fuel/params.rs");

// ─── Minimal JSON value extractor (no deps) ───────────────────────────────────

/// Extract `"key": VALUE` from a JSON string, returning VALUE as a &str.
/// Handles number, bool, and null.  Panics if key is absent.
fn json_value<'a>(json: &'a str, key: &str) -> &'a str {
    let pattern = format!("\"{key}\"");
    let pos = json.find(pattern.as_str())
        .unwrap_or_else(|| panic!("key {key:?} not found in JSON"));
    let after = &json[pos + pattern.len()..];
    // Skip whitespace and colon
    let after = after.trim_start_matches(|c: char| c.is_whitespace() || c == ':').trim_start();
    // Find the end of the value (comma, newline, '}', ']')
    let end = after.find(|c: char| matches!(c, ',' | '\n' | '}' | ']'))
        .unwrap_or(after.len());
    after[..end].trim()
}

/// Return the `payoutTerms.stepBps` value from a keeper.json string.
fn step_bps(keeper_json: &str) -> u64 {
    // payoutTerms block must contain stepBps
    let pos = keeper_json.find("\"payoutTerms\"")
        .expect("payoutTerms key missing");
    let block = &keeper_json[pos..];
    json_value(block, "stepBps").parse::<u64>().expect("stepBps not a number")
}

fn state_len(keeper_json: &str) -> u64 {
    json_value(keeper_json, "stateLen").parse::<u64>().expect("stateLen not a number")
}

fn version(keeper_json: &str) -> u64 {
    json_value(keeper_json, "version").parse::<u64>().expect("version not a number")
}

fn hook_accounts(keeper_json: &str) -> bool {
    json_value(keeper_json, "hookAccounts") == "true"
}

fn window_bps(keeper_json: &str) -> u64 {
    let pos = keeper_json.find("\"payoutTerms\"").expect("payoutTerms");
    let block = &keeper_json[pos..];
    json_value(block, "windowBps").parse::<u64>().expect("windowBps not a number")
}

// ─── params.rs source text checks ────────────────────────────────────────────

fn params_src_contains_payout_bps(src: &str, value: u32) -> bool {
    src.contains(&format!("payout_bps: {}_000,", value / 1000))
        || src.contains(&format!("payout_bps: {},", value))
}

fn params_src_contains_pace_steps(src: &str, value: u32) -> bool {
    src.contains(&format!("pace_steps: {},", value))
}

fn params_src_needs_hook(src: &str) -> bool {
    // Look for `needs_hook: true,`
    src.contains("needs_hook: true,")
}

// ─── D1: MOON ────────────────────────────────────────────────────────────────

#[test]
fn moon_keeper_step_bps_is_2000() {
    assert_eq!(step_bps(MOON_KEEPER), 2000, "MOON stepBps");
}

#[test]
fn moon_params_payout_bps_is_2000() {
    assert!(
        MOON_PARAMS_SRC.contains("payout_bps: 2_000,"),
        "MOON params.rs should have payout_bps: 2_000,"
    );
}

#[test]
fn moon_params_pace_steps_is_480() {
    assert!(
        params_src_contains_pace_steps(MOON_PARAMS_SRC, 480),
        "MOON params.rs should have pace_steps: 480"
    );
}

#[test]
fn moon_params_needs_hook_true() {
    assert!(params_src_needs_hook(MOON_PARAMS_SRC), "MOON needs_hook should be true");
}

// ─── D2: WICK ────────────────────────────────────────────────────────────────

#[test]
fn wick_keeper_step_bps_is_1000() {
    assert_eq!(step_bps(WICK_KEEPER), 1000, "WICK stepBps");
}

#[test]
fn wick_params_payout_bps_is_1000() {
    assert!(
        WICK_PARAMS_SRC.contains("payout_bps: 1_000,"),
        "WICK params.rs should have payout_bps: 1_000,"
    );
}

#[test]
fn wick_params_pace_steps_is_240() {
    assert!(
        params_src_contains_pace_steps(WICK_PARAMS_SRC, 240),
        "WICK params.rs should have pace_steps: 240"
    );
}

#[test]
fn wick_params_needs_hook_true() {
    assert!(params_src_needs_hook(WICK_PARAMS_SRC), "WICK needs_hook should be true");
}

// ─── D3: FUEL ────────────────────────────────────────────────────────────────

#[test]
fn fuel_keeper_step_bps_is_1000() {
    assert_eq!(step_bps(FUEL_KEEPER), 1000, "FUEL stepBps");
}

#[test]
fn fuel_params_payout_bps_is_1000() {
    assert!(
        FUEL_PARAMS_SRC.contains("payout_bps: 1_000,"),
        "FUEL params.rs should have payout_bps: 1_000,"
    );
}

#[test]
fn fuel_params_pace_steps_is_240() {
    assert!(
        params_src_contains_pace_steps(FUEL_PARAMS_SRC, 240),
        "FUEL params.rs should have pace_steps: 240"
    );
}

#[test]
fn fuel_params_needs_hook_false() {
    assert!(
        FUEL_PARAMS_SRC.contains("needs_hook: false,"),
        "FUEL needs_hook should be false"
    );
    assert!(!params_src_needs_hook(FUEL_PARAMS_SRC), "FUEL needs_hook should not be true");
}

// ─── D4: Shared keeper fields ─────────────────────────────────────────────────

#[test]
fn all_keepers_state_len_is_128() {
    assert_eq!(state_len(MOON_KEEPER), 128, "MOON stateLen");
    assert_eq!(state_len(WICK_KEEPER), 128, "WICK stateLen");
    assert_eq!(state_len(FUEL_KEEPER), 128, "FUEL stateLen");
    // The account is the discriminator plus the struct: the keeper's stateLen must match it.
    assert_eq!(
        8 + <workshop_script::ScriptState as anchor_lang::Space>::INIT_SPACE,
        128,
        "stateLen must equal the ScriptState account size"
    );
}

#[test]
fn all_keepers_version_is_1() {
    assert_eq!(version(MOON_KEEPER), 1, "MOON version");
    assert_eq!(version(WICK_KEEPER), 1, "WICK version");
    assert_eq!(version(FUEL_KEEPER), 1, "FUEL version");
}

#[test]
fn all_keepers_window_bps_is_5000() {
    assert_eq!(window_bps(MOON_KEEPER), 5000, "MOON windowBps");
    assert_eq!(window_bps(WICK_KEEPER), 5000, "WICK windowBps");
    assert_eq!(window_bps(FUEL_KEEPER), 5000, "FUEL windowBps");
}

// ─── D5: MOON is exactly 2× WICK/FUEL ────────────────────────────────────────

#[test]
fn moon_payout_bps_is_2x_wick() {
    assert_eq!(step_bps(MOON_KEEPER), 2 * step_bps(WICK_KEEPER), "MOON=2*WICK");
}

#[test]
fn wick_and_fuel_payout_bps_equal() {
    assert_eq!(step_bps(WICK_KEEPER), step_bps(FUEL_KEEPER), "WICK==FUEL stepBps");
}

// ─── D6: keeper stepBps matches params payout_bps ────────────────────────────

#[test]
fn moon_keeper_matches_params() {
    let keeper_step_bps = step_bps(MOON_KEEPER);
    assert!(
        MOON_PARAMS_SRC.contains(&format!("payout_bps: {}_000,", keeper_step_bps / 1000))
            || MOON_PARAMS_SRC.contains(&format!("payout_bps: {},", keeper_step_bps)),
        "MOON keeper stepBps={keeper_step_bps} not found in params.rs"
    );
}

#[test]
fn wick_keeper_matches_params() {
    let keeper_step_bps = step_bps(WICK_KEEPER);
    assert!(
        WICK_PARAMS_SRC.contains(&format!("payout_bps: {}_000,", keeper_step_bps / 1000))
            || WICK_PARAMS_SRC.contains(&format!("payout_bps: {},", keeper_step_bps)),
        "WICK keeper stepBps={keeper_step_bps} not found in params.rs"
    );
}

#[test]
fn fuel_keeper_matches_params() {
    let keeper_step_bps = step_bps(FUEL_KEEPER);
    assert!(
        FUEL_PARAMS_SRC.contains(&format!("payout_bps: {}_000,", keeper_step_bps / 1000))
            || FUEL_PARAMS_SRC.contains(&format!("payout_bps: {},", keeper_step_bps)),
        "FUEL keeper stepBps={keeper_step_bps} not found in params.rs"
    );
}

// ─── Keeper field presence (validator) ────────────────────────────────────────

#[test]
fn all_keepers_have_required_fields() {
    let required = [
        "version", "stateLen", "stepSecs", "payoutTerms",
        "stepBps", "holdingBps", "windowBps", "windowSecs",
        "launcherShareBps", "wallets",
    ];
    for (name, json) in [("MOON", MOON_KEEPER), ("WICK", WICK_KEEPER), ("FUEL", FUEL_KEEPER)] {
        for field in required {
            assert!(
                json.contains(&format!("\"{field}\"")),
                "{name} keeper.json missing required field {field:?}"
            );
        }
    }
}

#[test]
fn all_keepers_launcher_share_bps_is_5000() {
    for (name, json) in [("MOON", MOON_KEEPER), ("WICK", WICK_KEEPER), ("FUEL", FUEL_KEEPER)] {
        let v = json_value(json, "launcherShareBps").parse::<u64>().expect("launcherShareBps");
        assert_eq!(v, 5000, "{name} launcherShareBps");
    }
}

#[test]
fn all_keepers_holding_bps_is_10000() {
    for (name, json) in [("MOON", MOON_KEEPER), ("WICK", WICK_KEEPER), ("FUEL", FUEL_KEEPER)] {
        let pos = json.find("\"payoutTerms\"").expect("payoutTerms");
        let block = &json[pos..];
        let v = json_value(block, "holdingBps").parse::<u64>().expect("holdingBps");
        assert_eq!(v, 10000, "{name} holdingBps");
    }
}

#[test]
fn all_keepers_step_secs_is_60() {
    for (name, json) in [("MOON", MOON_KEEPER), ("WICK", WICK_KEEPER), ("FUEL", FUEL_KEEPER)] {
        let v = json_value(json, "stepSecs").parse::<u64>().expect("stepSecs");
        assert_eq!(v, 60, "{name} stepSecs");
    }
}

// ─── Diff: only differing fields ──────────────────────────────────────────────

#[test]
fn only_fuel_is_dip_only() {
    assert!(MOON_PARAMS_SRC.contains("dip_only: false,"), "MOON buys at its pace");
    assert!(WICK_PARAMS_SRC.contains("dip_only: false,"), "WICK buys at its pace");
    assert!(FUEL_PARAMS_SRC.contains("dip_only: true,"), "FUEL buys only dips");
}

#[test]
fn dip_step_secs_matches_keeper_step_secs() {
    let secs = json_value(FUEL_KEEPER, "stepSecs").parse::<i64>().expect("stepSecs");
    assert_eq!(workshop_script::params::DIP.step_secs, secs, "DIP.step_secs vs keeper stepSecs");
}

#[test]
fn wick_and_fuel_differ_only_in_needs_hook_and_dip_only() {
    // WICK has `needs_hook: true`, FUEL has `needs_hook: false`; FUEL alone is dip-only.
    // All other params should be equal.
    assert!(params_src_needs_hook(WICK_PARAMS_SRC), "WICK needs_hook=true");
    assert!(!params_src_needs_hook(FUEL_PARAMS_SRC), "FUEL needs_hook=false");
    // Step bps equal
    assert_eq!(step_bps(WICK_KEEPER), step_bps(FUEL_KEEPER), "stepBps equal");
    // pace_steps equal
    assert!(params_src_contains_pace_steps(WICK_PARAMS_SRC, 240));
    assert!(params_src_contains_pace_steps(FUEL_PARAMS_SRC, 240));
}

// ─── DIP (was FUEL 2; key fuel2): a second dip variant with its own constants ─────────────────────

const FUEL2_KEEPER: &str = include_str!("../../variants/fuel2/keeper.json");
const FUEL2_PARAMS_SRC: &str = include_str!("../../variants/fuel2/params.rs");

#[test]
fn fuel2_is_a_dip_only_script_with_a_five_minute_step() {
    assert!(FUEL2_PARAMS_SRC.contains("dip_only: true,"));
    assert!(FUEL2_PARAMS_SRC.contains("needs_hook: false,"));
    assert!(FUEL2_PARAMS_SRC.contains("pace_steps: 48,"));
    assert!(FUEL2_PARAMS_SRC.contains("trigger_bps: 800,"));
    assert!(FUEL2_PARAMS_SRC.contains("step_secs: 300,"));
    assert!(FUEL2_PARAMS_SRC.contains("payout_bps: 1_000,"));
    assert_eq!(json_value(FUEL2_KEEPER, "stepSecs").parse::<u64>().unwrap(), 300, "keeper stepSecs");
    assert_eq!(state_len(FUEL2_KEEPER), 128);
    assert_eq!(json_value(FUEL2_KEEPER, "launcherShareBps").parse::<u64>().unwrap(), 5000);
    assert_eq!(step_bps(FUEL2_KEEPER), 1000, "keeper stepBps matches payout_bps");
}

#[test]
fn fuel2_step_secs_matches_its_keeper() {
    // DIP.step_secs moves the dip reference once per step, so it must be the keeper's step.
    let secs = json_value(FUEL2_KEEPER, "stepSecs");
    assert!(FUEL2_PARAMS_SRC.contains(&format!("step_secs: {secs},")), "DIP.step_secs vs keeper stepSecs {secs}");
}
