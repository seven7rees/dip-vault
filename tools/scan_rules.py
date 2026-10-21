"""Static scan of submission sources against the Workshop's published template rules.

Implements the rules in SPEC.md F13/F14 (taken from the three Workshop briefs). It blanks comments and
strings first, like the Workshop's reading does, then looks for forbidden constructs.

usage: python3 -I scan_rules.py <dist_dir>
exit 0 when clean, 1 when any rule is broken.
"""
import json
import re
import sys
from pathlib import Path

MAX_FILES, MAX_BYTES = 8, 120_000
KEEPER_MAX, HOOK_MAX = 4_096, 1_024

COMMON = {
    "unsafe": r"\bunsafe\b",
    "asm!": r"\basm!",
    "macro_rules!": r"\bmacro_rules!",
    "cfg_attr": r"\bcfg_attr\b",
    "extern": r"\bextern\b",
    "no_mangle": r"\bno_mangle\b",
    "link attr": r"#\[\s*link\b",
    "include_*!": r"\binclude_(str|bytes)?!|\binclude!",
    "env!": r"\benv!",
    "option_env!": r"\boption_env!",
    "#[path]": r"#\[\s*path\b",
    "std::fs": r"std::fs\b",
    "std::net": r"std::net\b",
    "std::process": r"std::process\b",
    "std::os": r"std::os\b",
    "std::env": r"std::env\b",
    "std::thread": r"std::thread\b",
}
HOOK_ONLY = {
    "invoke": r"\binvoke(_signed)?\b",
    "CpiContext": r"\bCpiContext\b",
    "cpi module": r"\bcpi::",
    "close account": r"\bclose\s*=",
    "realloc": r"\brealloc\b",
    "lamports mutation": r"\bsub_lamports\b|\badd_lamports\b|\*\*[a-z_.]*lamports\.borrow_mut",
}


def blank(src: str) -> str:
    """Blank comments, strings and char literals, keeping line structure."""
    out, i, n = [], 0, len(src)
    while i < n:
        c = src[i]
        if src.startswith("//", i):
            j = src.find("\n", i)
            j = n if j < 0 else j
            out.append(" " * (j - i))
            i = j
        elif src.startswith("/*", i):
            depth, j = 1, i + 2
            while j < n and depth:
                if src.startswith("/*", j):
                    depth, j = depth + 1, j + 2
                elif src.startswith("*/", j):
                    depth, j = depth - 1, j + 2
                else:
                    j += 1
            out.append(re.sub(r"[^\n]", " ", src[i:j]))
            i = j
        elif c == '"':
            j = i + 1
            while j < n and src[j] != '"':
                j += 2 if src[j] == "\\" else 1
            out.append(re.sub(r"[^\n]", " ", src[i : j + 1]))
            i = j + 1
        else:
            out.append(c)
            i += 1
    return "".join(out)


def scan_half(root: Path, hook: bool, problems: list):
    files = sorted((root / "src").glob("**/*.rs"))
    total = sum(f.stat().st_size for f in files)
    if not files:
        problems.append(f"{root}: no src/*.rs")
    if len(files) > MAX_FILES:
        problems.append(f"{root}: {len(files)} files, max {MAX_FILES}")
    if total > MAX_BYTES:
        problems.append(f"{root}: {total} bytes, max {MAX_BYTES}")
    code_all = ""
    for f in files:
        if not re.fullmatch(r"[a-z0-9_]+\.rs", f.name):
            problems.append(f"{f}: file name must be lower-case")
        code = blank(f.read_text(encoding="utf-8"))
        code_all += code
        rules = dict(COMMON, **(HOOK_ONLY if hook else {}))
        for name, pat in rules.items():
            for m in re.finditer(pat, code):
                line = code.count("\n", 0, m.start()) + 1
                problems.append(f"{f}:{line}: forbidden `{name}`")
    for name, pat, want in (("declare_id!", r"\bdeclare_id!", 1), ("#[program]", r"#\[\s*program\s*\]", 1)):
        got = len(re.findall(pat, code_all))
        if got != want:
            problems.append(f"{root}: {got} x {name}, need exactly {want}")
    if hook and not re.search(r"fn\s+prepare\s*\(", code_all):
        problems.append(f"{root}: no `prepare`")
    return total


# keeper.json: the SDK's validator (shared/src/keeper.ts) allows ROOT_KEYS; the live docs
# (bordrless.app/sdk/companions) add the three LIVE_KEYS. Ranges are the companion's.
ROOT_KEYS = {"version", "holdings", "swapAccounts", "hookAccounts", "payouts", "roundSecs", "wallets", "stepSecs", "stateLen", "initData"}
LIVE_KEYS = {"mintWritable", "payoutTerms", "launcherShareBps", "view", "pools"}
STATE_LEN = 128  # 8-byte discriminator + ScriptState::INIT_SPACE (120); T7 and T8 pin the same number
STATE_LENS = {"dipvault": 320}  # pairs with their own state layout (T19 pins it)
B58 = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"


PROTOCOL_KEYS = {
    "11111111111111111111111111111111",
    "6ZUM1gWBH9hBBNoJoaVAGwSftyZ6CUda6vUZTW9MsJuo",  # companion
    "2XoEWp8cF3kRXg74eVwPAyTFhVCAztn3V88komxAvr22",  # token
    "GyzKSnnEu2uN5bBRecE4XYY2enbfR2D2MtxnbJPGy7hk",  # DEX
    "1jcBymHxBjniZDhNPy51Vgm5Nz7pLUdxa9UBHc4TavC",  # launchpad
}


def is_int(x) -> bool:
    return isinstance(x, int) and not isinstance(x, bool)


def is_pubkey(s) -> bool:
    """A base58 string that decodes to exactly 32 bytes and is not a protocol address."""
    if not isinstance(s, str) or not s or any(c not in B58 for c in s) or s in PROTOCOL_KEYS:
        return False
    n = 0
    for c in s:
        n = n * 58 + B58.index(c)
    lead = len(s) - len(s.lstrip("1"))
    body = n.to_bytes(max(1, (n.bit_length() + 7) // 8), "big") if n else b""
    return lead + len(body) == 32


def check_keeper(path: Path, problems: list, state_len: int = STATE_LEN):
    try:
        k = json.loads(path.read_text())
    except ValueError as e:
        problems.append(f"{path}: not JSON ({e})")
        return
    if not isinstance(k, dict):
        problems.append(f"{path}: not an object")
        return
    extra = set(k) - ROOT_KEYS - LIVE_KEYS
    if extra:
        problems.append(f"{path}: unknown keys {sorted(extra)}")
    if k.get("version") != 1:
        problems.append(f"{path}: version must be 1")
    if "swapAccounts" not in k:
        problems.append(f"{path}: swapAccounts is required")
    for flag in ("swapAccounts", "hookAccounts", "payouts", "mintWritable"):
        if flag in k and not isinstance(k[flag], bool):
            problems.append(f"{path}: {flag} must be true or false")
    wallets = k.get("wallets") or []
    if not isinstance(wallets, list) or len(wallets) > 12 or len(set(wallets)) != len(wallets) or not all(is_pubkey(w) for w in wallets):
        problems.append(f"{path}: wallets must be at most 12 distinct base58 keys")
    if k.get("holdings") is None and not wallets:
        problems.append(f"{path}: holdings is required unless wallets are listed")
    step = k.get("stepSecs")
    if step is not None and not (is_int(step) and 10 <= step <= 86_400):
        problems.append(f"{path}: stepSecs must be a whole 10 to 86,400")
    sl = k.get("stateLen")
    if sl is not None and not (is_int(sl) and 0 <= sl <= 10_240):
        problems.append(f"{path}: stateLen must be 0 to 10,240")
    if sl != state_len:
        problems.append(f"{path}: stateLen {sl} is not the ScriptState account size {state_len}")
    pt = k.get("payoutTerms")
    if pt is not None:
        ok = isinstance(pt, dict) and set(pt) == {"stepBps", "holdingBps", "windowBps", "windowSecs"}
        ok = ok and all(is_int(pt[x]) and 1 <= pt[x] <= 10_000 for x in ("stepBps", "holdingBps", "windowBps"))
        ok = ok and is_int(pt["windowSecs"]) and 10 <= pt["windowSecs"] <= 86_400
        if not ok:
            problems.append(f"{path}: payoutTerms needs stepBps, holdingBps, windowBps in 1..10000 and windowSecs in 10..86400")
    ls = k.get("launcherShareBps")
    if ls is not None and not (is_int(ls) and 0 <= ls <= 9_500):
        problems.append(f"{path}: launcherShareBps must be 0 to 9,500")
    if k.get("mintWritable") is not True:
        problems.append(f"{path}: mintWritable must be true (the burn needs the mint writable)")


def main():
    dist = Path(sys.argv[1])
    problems = []
    for variant in sorted(p for p in dist.iterdir() if p.is_dir()):
        hook, script = variant / "hook", variant / "script"
        if hook.is_dir():
            scan_half(hook, True, problems)
            hj = hook / "hook.json"
            if not hj.is_file():
                problems.append(f"{hook}: hook.json missing")
            else:
                if hj.stat().st_size > HOOK_MAX:
                    problems.append(f"{hj}: over {HOOK_MAX} bytes")
                q = json.loads(hj.read_text())["quoteCut"]
                if not (1 <= q["maxBuyBps"] <= 1000 and 0 <= q["maxSellBps"] <= 1000):
                    problems.append(f"{hj}: caps out of bounds")
        if script.is_dir():
            scan_half(script, False, problems)
            kj = script / "keeper.json"
            if not kj.is_file():
                problems.append(f"{script}: keeper.json missing")
            elif kj.stat().st_size > KEEPER_MAX:
                problems.append(f"{kj}: over {KEEPER_MAX} bytes")
            else:
                check_keeper(kj, problems, STATE_LENS.get(variant.name, STATE_LEN))
            if not (script / "src" / "lib.rs").is_file():
                problems.append(f"{script}: src/lib.rs missing")
        print(f"{variant.name}: scanned")
    for p in problems:
        print("PROBLEM", p)
    print("clean" if not problems else f"{len(problems)} problem(s)")
    sys.exit(1 if problems else 0)


main()
