"""Build dist/<variant>/ from the template crates, one directory per Workshop submission.

hook/ and script/ are the template (MOON values). A variant differs only in script/src/params.rs
(three numbers and the hook flag), keeper.json (stepBps) and the builder's words. The wallet comes from
WALLET.txt. Writes variants/<name>/ (what differs) and dist/<name>/ (what is submitted), then checks the
Workshop's size limits.

usage: python3 -I make_variants.py
"""
import json
import re
import shutil
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
WALLET = (ROOT / "WALLET.txt").read_text().strip()

VARIANTS = {
    # name: (payout_bps, pace_steps, needs_hook, step_bps, ships_hook, notes block title, dip_only)
    "moon": (2_000, 480, True, 2_000, True, "MOON notes", False),
    "wick": (1_000, 240, True, 1_000, True, "WICK notes", False),
    "fuel": (1_000, 240, False, 1_000, False, "FUEL notes", True),
    # DIP (item name, decided 2026-10-17; internal key stays fuel2): fewer, bigger, more selective dip buys; steps every 5 minutes. Built to be
    # submitted with the author share set to None (the launcher then keeps half the creator fee).
    "fuel2": (1_000, 48, False, 1_000, False, "DIP notes", True),
}
# Per-variant settings beyond the tuple: the `DIP` struct values (script/src/params.rs) and keeper stepSecs.
# DIP.step_secs must equal keeper stepSecs (T8 checks it).
OVERRIDES = {
    "fuel2": {"dip": {"trigger_bps": 800, "step_secs": 300}, "step_secs": 300},
}
STATE_LEN = 128  # 8-byte discriminator + ScriptState::INIT_SPACE (120); same for every engine variant
# Pairs built from their own crates (not the shared engine): hook and script directories, the
# builder's words file and the script state size (keeper.json stateLen; a layout test pins it).
PAIRS = {
    "dipvault": {"hook": "hook_variants/dipvault", "script": "script_variants/dipvault", "state_len": 320},
}
LAUNCHER_SHARE_BPS = 5_000  # owner's decision, 2026-10-15; live docs only (/sdk/companions)
LIMITS = {"files": 8, "bytes": 120_000, "notes": 600, "name": 32}


def notes_blocks() -> dict:
    out, title = {}, None
    for line in (ROOT / "drafts" / "notes-v7.txt").read_text().splitlines():
        if line.startswith("### "):
            title = line[4:].strip()
            out[title] = ""
        elif title and line.strip():
            out[title] += line.strip()
    return out


def params_rs(name: str, payout: int, pace: int, needs_hook: bool, dip_only: bool, dip: dict) -> str:
    text = (ROOT / "script" / "src" / "params.rs").read_text()
    text = re.sub(r"payout_bps: [\d_]+,", f"payout_bps: {payout:_},".replace("_", "_"), text, count=1)
    text = re.sub(r"pace_steps: \d+,", f"pace_steps: {pace},", text, count=1)
    text = re.sub(r"needs_hook: (true|false),", f"needs_hook: {str(needs_hook).lower()},", text, count=1)
    text = re.sub(r"dip_only: (true|false),", f"dip_only: {str(dip_only).lower()},", text, count=1)
    for key, val in dip.items():
        text, n = re.subn(rf"(\b{key}: )[\d_]+,", rf"\g<1>{val},", text, count=1)
        if n != 1:
            sys.exit(f"{name}: DIP.{key} not found in script/src/params.rs")
    text = re.sub(r'(BUILDER_WALLET: Pubkey =\s*Pubkey::from_str_const\(\s*")[1-9A-HJ-NP-Za-km-z]{32,44}(")', rf"\g<1>{WALLET}\g<2>", text)
    if WALLET not in text:
        sys.exit("BUILDER_WALLET literal not found in script/src/params.rs")
    return text


def main():
    notes = notes_blocks()
    problems = []
    dist = ROOT / "dist"
    if dist.exists():
        shutil.rmtree(dist)
    for name, (payout, pace, needs_hook, step_bps, ships_hook, title, dip_only) in VARIANTS.items():
        var = ROOT / "variants" / name
        var.mkdir(parents=True, exist_ok=True)
        over = OVERRIDES.get(name, {})
        params = params_rs(name, payout, pace, needs_hook, dip_only, over.get("dip", {}))
        keeper = json.loads((ROOT / "script" / "keeper.json").read_text())
        keeper["wallets"] = [WALLET]
        keeper["hookAccounts"] = False
        keeper["payoutTerms"]["stepBps"] = step_bps
        keeper["launcherShareBps"] = LAUNCHER_SHARE_BPS
        keeper["stateLen"] = STATE_LEN
        if "step_secs" in over:
            keeper["stepSecs"] = over["step_secs"]
        wallet_in_params = re.search(r'BUILDER_WALLET: Pubkey =\s*Pubkey::from_str_const\(\s*"([1-9A-HJ-NP-Za-km-z]+)"', params).group(1)
        if wallet_in_params not in keeper["wallets"]:
            sys.exit(f"{name}: keeper.json wallets must include params.rs BUILDER_WALLET ({wallet_in_params})")
        (var / "params.rs").write_text(params)
        (var / "keeper.json").write_text(json.dumps(keeper, indent=2) + "\n")
        (var / "notes.txt").write_text(notes[title] + "\n")
        out = dist / name
        shutil.copytree(ROOT / "script", out / "script", ignore=shutil.ignore_patterns("target", "Cargo.lock"))
        (out / "script" / "src" / "params.rs").write_text(params)
        (out / "script" / "keeper.json").write_text(json.dumps(keeper, indent=2) + "\n")
        if ships_hook:
            shutil.copytree(ROOT / "hook", out / "hook", ignore=shutil.ignore_patterns("target", "Cargo.lock"))
        for half in ("hook", "script"):
            src = out / half / "src"
            if src.is_dir():
                files = list(src.glob("*.rs"))
                size = sum(f.stat().st_size for f in files)
                if len(files) > LIMITS["files"] or size > LIMITS["bytes"]:
                    problems.append(f"{name}/{half}: {len(files)} files, {size} bytes")
        if len(notes[title]) > LIMITS["notes"]:
            problems.append(f"{name}: notes {len(notes[title])} chars, max {LIMITS['notes']}")
        print(f"{name}: dist ready, notes {len(notes[title])} chars")
    for name, pair in PAIRS.items():
        var = ROOT / "variants" / name
        var.mkdir(parents=True, exist_ok=True)
        hook_dir, script_dir = ROOT / pair["hook"], ROOT / pair["script"]
        params = (script_dir / "src" / "params.rs").read_text()
        keeper = json.loads((script_dir / "keeper.json").read_text())
        if WALLET not in params or WALLET not in keeper.get("wallets", []):
            sys.exit(f"{name}: BUILDER_WALLET must be in params.rs and keeper.json wallets")
        if keeper.get("stateLen") != pair["state_len"]:
            sys.exit(f"{name}: keeper.json stateLen {keeper.get('stateLen')} != {pair['state_len']}")
        words = (var / "notes.txt").read_text().strip()
        (var / "params.rs").write_text(params)
        (var / "keeper.json").write_text(json.dumps(keeper, indent=2) + "\n")
        out = dist / name
        shutil.copytree(script_dir, out / "script", ignore=shutil.ignore_patterns("target", "Cargo.lock"))
        shutil.copytree(hook_dir, out / "hook", ignore=shutil.ignore_patterns("target", "Cargo.lock"))
        for half in ("hook", "script"):
            src = out / half / "src"
            files = list(src.glob("*.rs"))
            size = sum(f.stat().st_size for f in files)
            if len(files) > LIMITS["files"] or size > LIMITS["bytes"]:
                problems.append(f"{name}/{half}: {len(files)} files, {size} bytes")
        if len(words) > LIMITS["notes"]:
            problems.append(f"{name}: notes {len(words)} chars, max {LIMITS['notes']}")
        print(f"{name}: dist ready (pair), notes {len(words)} chars")
    for p in problems:
        print("PROBLEM", p)
    sys.exit(1 if problems else 0)


main()
