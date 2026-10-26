"""Which models can the training experiments use right now? (specs/037 section 2, OD-T2)

usage:
  python3 -I dev/train/avail.py [--home DIR] [--workspace NAME] [--live] [--json OUT]
                                [--claude-models a,b,c] [--only claude,agy,ollama,api]
                                [--claude-bin P] [--agy-bin P] [--ollama URL]
  python3 -I dev/train/avail.py self-test

Three layers, cheapest first, and a model is only called when nothing cheaper can answer:

  1. offline     the workspace's configured tiers, the saved pool state (a model marked exhausted until a time) and the
                 last capability probe. No process is started, nothing is sent.
  2. deterministic  `claude auth status`, `agy --version` and `agy models` (the account's model catalogue), Ollama's
                 /api/tags, and the model-list endpoint of any API provider whose key is already in the environment.
                 These answer "is it installed, signed in, and does the account offer this model", never "is there quota
                 left": a listing cannot see a quota.
  3. live (--live)  one tiny call per model, in parallel, only to learn about quota: the Claude CLI's stream reports its
                 five-hour and seven-day utilization for free with the call; agy reports `quota reached ... Resets in 3h`.
                 A call uses a few thousand cached input tokens and one output token.

Output is a table, and with --json a file other tools read (`delphi train arms init` takes it). Statuses:
  available     a live call succeeded (or, without --live, the catalogue lists it and no exhaustion is recorded)
  exhausted     quota reached; `until` says when it resets
  unlisted      the account's catalogue does not offer it
  unauthenticated, unreachable, error   with the reason
  unknown       listed but not called (no --live) and no exhaustion on record
  skipped       an API provider with no key in the environment

Stdlib only; reads the Delphi home read-only; never prints a credential.
"""
import argparse
import concurrent.futures
import datetime
import json
import os
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile
import time
import urllib.request

CLAUDE_DEFAULTS = ["haiku", "sonnet", "opus", "fable"]
PROBE_PROMPT = "Reply with exactly: OK"
LIVE_TIMEOUT = 90


# ── small helpers ───────────────────────────────────────────────────────────────────────────────────────────────

def run(cmd, timeout=30):
    """(exit code, stdout, stderr, seconds); a missing program or a timeout is code None."""
    t0 = time.time()
    try:
        p = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout, stdin=subprocess.DEVNULL)
        return p.returncode, p.stdout, p.stderr, time.time() - t0
    except FileNotFoundError:
        return None, "", "not installed", time.time() - t0
    except subprocess.TimeoutExpired as e:
        return None, (e.stdout or b"").decode() if isinstance(e.stdout, bytes) else (e.stdout or ""), "timed out after %ds" % timeout, time.time() - t0


def http_json(url, headers=None, timeout=10):
    req = urllib.request.Request(url, headers=headers or {})
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return json.loads(r.read().decode())


def duration_secs(text):
    """`101h32m27s`, `3h`, `45m` or `90s` as seconds; None if no such span is in the text."""
    m = re.search(r"(?:(\d+)\s*d)?\s*(?:(\d+)\s*h)?\s*(?:(\d+)\s*m(?!s))?\s*(?:(\d+)\s*s)?", text or "")
    if not m or not any(m.groups()):
        return None
    d, h, mi, s = (int(g) if g else 0 for g in m.groups())
    return d * 86400 + h * 3600 + mi * 60 + s


def iso(ts):
    return datetime.datetime.fromtimestamp(ts, datetime.timezone.utc).strftime("%Y-%m-%d %H:%MZ")


def row(provider, model, status, detail="", label="", tiers=None, source="", until=None, latency=None, windows=None):
    return {"provider": provider, "model": model, "label": label, "tiers": tiers or [], "status": status, "detail": detail,
            "source": source, "until": until, "latency_s": None if latency is None else round(latency, 1), "windows": windows or {}}


# ── layer 1: offline ────────────────────────────────────────────────────────────────────────────────────────────

def parse_config(text):
    """The few things the workspace config says that matter here, without a TOML library (Python 3.9 has none).
    Returns {"endpoints": [{id, family, path, harness, base_url, credential, models}], "tiers": {family: {tier: model}}}."""
    endpoints, tiers, cur, kind = [], {}, None, None
    for raw in text.splitlines():
        line = raw.split("#", 1)[0].strip() if not raw.strip().startswith('"') else raw.strip()
        if not line:
            continue
        m = re.match(r"^\[\[(.+)\]\]$", line)
        if m:
            kind = m.group(1).strip()
            cur = {"models": {}} if kind == "endpoints" else None
            if cur is not None:
                endpoints.append(cur)
            continue
        m = re.match(r"^\[(.+)\]$", line)
        if m:
            name = m.group(1).strip()
            kind, cur = name, None
            mm = re.match(r"^models\.([A-Za-z0-9_-]+)$", name)
            if mm:
                cur = tiers.setdefault(mm.group(1), {})
            elif name == "endpoints.models" and endpoints:
                cur = endpoints[-1]["models"]
            continue
        kv = re.match(r'^([A-Za-z0-9_.-]+)\s*=\s*(.+)$', line)
        if not kv or cur is None:
            continue
        key, val = kv.group(1), kv.group(2).strip()
        sm = re.match(r'^"([^"]*)"$', val)
        if sm:
            cur[key] = sm.group(1)
        elif key == "models" and val.startswith("{") and isinstance(cur, dict) and "models" in cur:
            for k2, v2 in re.findall(r'([A-Za-z0-9_-]+)\s*=\s*"([^"]*)"', val):
                cur["models"][k2] = v2
    return {"endpoints": [e for e in endpoints if e.get("id")], "tiers": {f: {k: v for k, v in t.items() if not v.startswith("<")} for f, t in tiers.items()}}


def tiers_of(cfg):
    """{model id: ["claude:fast", ...]}: what the workspace calls each model, from the family tiers and the endpoints."""
    out = {}
    for fam, t in cfg["tiers"].items():
        for tier, m in t.items():
            out.setdefault(m, []).append("%s:%s" % (fam, tier))
    for e in cfg["endpoints"]:
        for tier, m in e.get("models", {}).items():
            out.setdefault(m, []).append("%s:%s" % (e.get("family", e["id"]), tier))
    return {m: sorted(set(v)) for m, v in out.items()}


def read_json_file(path):
    try:
        return json.loads(pathlib.Path(path).read_text())
    except (OSError, ValueError):
        return None


def offline_state(ws):
    """Exhaustion marks from the saved pool: {(endpoint, model): until}."""
    st = read_json_file(ws / "local" / "pool-state.json") or {}
    return {(e.get("endpoint_id"), e.get("model_id")): e for e in st.get("entries", []) if e.get("state") == "exhausted"}


# ── layer 2: deterministic ──────────────────────────────────────────────────────────────────────────────────────

def check_claude(bin_, wanted, tiers):
    """The Claude CLI: installed, signed in. Its models have no listing, so each wanted alias is `unknown` until --live."""
    code, out, err, _ = run([bin_, "--version"])
    if code is None:
        return [row("claude", m, "unreachable", "the `claude` CLI is not installed", tiers=tiers.get(m), source="deterministic") for m in wanted], None
    version = out.strip()
    code, out, err, _ = run([bin_, "auth", "status"])
    try:
        auth = json.loads(out)
    except ValueError:
        auth = {}
    if not auth.get("loggedIn"):
        return [row("claude", m, "unauthenticated", "run `claude auth login`", tiers=tiers.get(m), source="deterministic") for m in wanted], version
    who = "%s (%s)" % (auth.get("authMethod", "?"), auth.get("apiProvider", "?"))
    return [row("claude", m, "unknown", "signed in via %s; Claude CLI %s; no model listing, use --live for quota" % (who, version), tiers=tiers.get(m), source="deterministic") for m in wanted], version


def parse_agy_models(text):
    """`agy models` prints `id<TAB>Label` lines after a progress line."""
    out = []
    for line in text.splitlines():
        if "\t" in line:
            mid, label = line.split("\t", 1)
            out.append((mid.strip(), label.strip()))
    return out


def check_agy(bin_, tiers):
    code, out, err, _ = run([bin_, "--version"])
    if code is None:
        return [row("agy", "(cli)", "unreachable", "the `agy` CLI is not installed", source="deterministic")]
    version = out.strip()
    code, out, err, _ = run([bin_, "models"], timeout=45)
    models = parse_agy_models(out)
    if not models:
        return [row("agy", "(catalogue)", "error", ("agy models listed nothing: " + (err.strip() or out.strip())[:120]).strip(), source="deterministic")]
    return [row("agy", mid, "unknown", "listed by the account; agy %s" % version, label=label, tiers=tiers.get(mid), source="deterministic") for mid, label in models]


def check_ollama(base):
    try:
        tags = http_json(base.rstrip("/") + "/api/tags", timeout=3)
    except Exception as e:  # noqa: BLE001
        return [row("ollama", "(server)", "unreachable", "%s: %s" % (base, str(e)[:80]), source="deterministic")]
    out = []
    for m in tags.get("models", []):
        name = m.get("name", "")
        if "embed" in name:
            continue
        d = m.get("details", {})
        out.append(row("ollama", name, "unknown", "installed locally; %s %s" % (d.get("parameter_size", "?"), d.get("quantization_level", "")), source="deterministic"))
    return out


API_PROVIDERS = [
    # (provider, env vars tried, model-list URL, auth header builder, id extractor)
    ("anthropic-api", ["ANTHROPIC_API_KEY"], "https://api.anthropic.com/v1/models?limit=100",
     lambda k: {"x-api-key": k, "anthropic-version": "2023-06-01"}, lambda j: [m["id"] for m in j.get("data", [])]),
    ("gemini-api", ["GEMINI_API_KEY", "GOOGLE_API_KEY"], "https://generativelanguage.googleapis.com/v1beta/models?pageSize=200",
     lambda k: {"x-goog-api-key": k}, lambda j: [m["name"].split("/")[-1] for m in j.get("models", [])]),
    ("openai-api", ["OPENAI_API_KEY"], "https://api.openai.com/v1/models",
     lambda k: {"authorization": "Bearer " + k}, lambda j: [m["id"] for m in j.get("data", [])]),
]


def check_api(env):
    """A model-list endpoint is a deterministic availability check that costs nothing, for a provider whose key is in the
    environment. (Subscription seats have no such endpoint.)"""
    out = []
    for name, vars_, url, hdr, ids in API_PROVIDERS:
        key = next((env[v] for v in vars_ if env.get(v)), None)
        if not key:
            out.append(row(name, "(key)", "skipped", "no %s in the environment" % "/".join(vars_), source="deterministic"))
            continue
        try:
            for mid in ids(http_json(url, hdr(key))):
                out.append(row(name, mid, "unknown", "listed by the provider's model endpoint", source="deterministic"))
        except Exception as e:  # noqa: BLE001
            out.append(row(name, "(list)", "error", str(e)[:100], source="deterministic"))
    return out


# ── layer 3: live ───────────────────────────────────────────────────────────────────────────────────────────────

def classify_claude_stream(text, now=None):
    """(status, detail, until, windows, tokens) from a `claude -p --output-format stream-json --verbose` stream. Utilization
    comes from the `rate_limit_event` the CLI emits with the call, at no extra cost."""
    now = now or time.time()
    windows, result, status_seen = {}, None, None
    for line in text.splitlines():
        try:
            v = json.loads(line)
        except ValueError:
            continue
        if v.get("type") == "rate_limit_event":
            info = v.get("rate_limit_info", {})
            status_seen = info.get("status", status_seen)
            for name, w in (info.get("unifiedWindows") or {}).items():
                windows[name] = {"used": w.get("utilization"), "resets_at": w.get("resetsAt"), "status": w.get("status")}
            if info.get("rateLimitType") and info.get("rateLimitType") not in windows:
                windows[info["rateLimitType"]] = {"used": info.get("utilization"), "resets_at": info.get("resetsAt"), "status": info.get("status")}
        if v.get("type") == "result":
            result = v
    until = None
    if status_seen == "rejected":
        rejected = [w.get("resets_at") for w in windows.values() if w.get("status") == "rejected" and w.get("resets_at")]
        until = max(rejected) if rejected else None
        return "exhausted", "rate limited" + (" until %s" % iso(until) if until else ""), until, windows, 0
    if result is None:
        return "error", "no result in the stream", None, windows, 0
    if result.get("is_error"):
        msg = str(result.get("result", ""))[:160]
        low = msg.lower()
        if "limit" in low or "quota" in low:
            m = re.search(r"\|(\d{9,11})", msg)
            until = int(m.group(1)) if m else None
            return "exhausted", msg, until, windows, 0
        if "model" in low and ("not found" in low or "invalid" in low or "does not exist" in low or "issue with the selected model" in low):
            return "unlisted", msg, None, windows, 0
        return "error", msg, None, windows, 0
    u = result.get("usage", {})
    toks = int(u.get("input_tokens", 0)) + int(u.get("cache_creation_input_tokens", 0)) + int(u.get("cache_read_input_tokens", 0)) + int(u.get("output_tokens", 0))
    return "available", "", None, windows, toks


def classify_agy_stream(text, now=None):
    """(status, detail, until, tokens) from `agy --print --output-format stream-json`: a result with status SUCCESS, or an
    error line such as `Individual quota reached ... Resets in 101h32m27s`."""
    now = now or time.time()
    for line in text.splitlines():
        try:
            v = json.loads(line)
        except ValueError:
            continue
        if v.get("event") == "result":
            r = v.get("result", {})
            if r.get("status") == "SUCCESS":
                return "available", "", None, int(r.get("usage", {}).get("total_tokens", 0))
    low = text.lower()
    if "quota" in low or "resource_exhausted" in low or "429" in low:
        m = re.search(r"resets in\s+([0-9dhms ]+)", text, re.I)
        secs = duration_secs(m.group(1)) if m else None
        first = next((l for l in text.splitlines() if l.lower().startswith("error:")), "quota reached")
        return "exhausted", first[:140], (int(now + secs) if secs else None), 0
    if "not found" in low or "unknown model" in low or "invalid model" in low:
        return "unlisted", (text.strip().splitlines() or [""])[-1][:140], None, 0
    if "login" in low or "authenticat" in low or "sign in" in low:
        return "unauthenticated", (text.strip().splitlines() or [""])[-1][:140], None, 0
    return "error", (text.strip().splitlines() or ["no result"])[-1][:140], None, 0


def live_claude(bin_, model):
    cmd = [bin_, "-p", PROBE_PROMPT, "--model", model, "--output-format", "stream-json", "--verbose", "--max-turns", "1", "--tools", "",
           "--system-prompt", "Reply briefly.", "--no-session-persistence", "--disable-slash-commands", "--strict-mcp-config"]
    code, out, err, secs = run(cmd, LIVE_TIMEOUT)
    if code is None:
        return "error", err, None, {}, secs, 0
    status, detail, until, windows, toks = classify_claude_stream(out + "\n" + err)
    return status, detail, until, windows, secs, toks


def live_agy(bin_, model):
    code, out, err, secs = run([bin_, "--print", PROBE_PROMPT, "--model", model, "--output-format", "stream-json"], LIVE_TIMEOUT)
    if code is None:
        return "error", err, None, {}, secs, 0
    status, detail, until, toks = classify_agy_stream(out + "\n" + err)
    return status, detail, until, {}, secs, toks


def live_ollama(base, model):
    t0 = time.time()
    try:
        req = urllib.request.Request(base.rstrip("/") + "/api/generate", data=json.dumps({"model": model, "prompt": PROBE_PROMPT, "stream": False, "options": {"num_predict": 4}}).encode(), headers={"content-type": "application/json"})
        with urllib.request.urlopen(req, timeout=LIVE_TIMEOUT) as r:
            json.loads(r.read().decode())
        return "available", "", None, {}, time.time() - t0, 0
    except Exception as e:  # noqa: BLE001
        return "error", str(e)[:100], None, {}, time.time() - t0, 0


# ── putting it together ─────────────────────────────────────────────────────────────────────────────────────────

def apply_exhaustion(rows, exhausted, now):
    """An unchecked model the saved pool says is exhausted, and whose time has not passed, is exhausted (from the pool, not
    from a call). A live result replaces it."""
    for r in rows:
        if r["status"] != "unknown":
            continue
        for (_, model), e in exhausted.items():
            if model == r["model"] and (e.get("until") or 0) > now:
                r.update(status="exhausted", until=e.get("until"), detail="the saved pool state marks it exhausted until %s (%s)" % (iso(e["until"]), e.get("reason", "")), source="offline")
    return rows


def run_live(rows, args):
    jobs = []
    for r in rows:
        if r["status"] in ("unreachable", "unauthenticated") or r["model"].startswith("("):
            continue
        if r["provider"] == "claude":
            jobs.append((r, lambda m=r["model"]: live_claude(args.claude_bin, m), "claude"))
        elif r["provider"] == "agy":
            jobs.append((r, lambda m=r["model"]: live_agy(args.agy_bin, m), "agy"))
        elif r["provider"] == "ollama":
            jobs.append((r, lambda m=r["model"]: live_ollama(args.ollama, m), "ollama"))
    # Claude calls share one subscription window: two at a time. agy and Ollama can go wider.
    pools = {"claude": concurrent.futures.ThreadPoolExecutor(2), "agy": concurrent.futures.ThreadPoolExecutor(4), "ollama": concurrent.futures.ThreadPoolExecutor(1)}
    futs = [(r, pools[p].submit(fn)) for r, fn, p in jobs]
    for r, f in futs:
        try:
            status, detail, until, windows, secs, toks = f.result()
        except Exception as e:  # noqa: BLE001
            status, detail, until, windows, secs, toks = "error", str(e)[:100], None, {}, None, 0
        r.update(status=status, source="live", until=until, latency_s=None if secs is None else round(secs, 1), windows=windows)
        r["detail"] = detail or ("answered in %.1fs, %d tokens" % (secs or 0, toks))
        if windows:
            r["detail"] += "; " + ", ".join("%s %.0f%% used" % (k, 100 * (w.get("used") or 0)) for k, w in sorted(windows.items()))
    for p in pools.values():
        p.shutdown()
    return rows


def survey(args):
    home = pathlib.Path(os.path.expanduser(args.home))
    ws = home / "workspaces" / args.workspace
    cfg = parse_config((ws / "config.toml").read_text()) if (ws / "config.toml").is_file() else {"endpoints": [], "tiers": {}}
    tiers = tiers_of(cfg)
    only = set(args.only.split(",")) if args.only else {"claude", "agy", "ollama", "api"}
    wanted = [m for m in args.claude_models.split(",") if m]
    for m in tiers:
        if any(t.startswith("claude:") for t in tiers[m]) and m not in wanted:
            wanted.append(m)
    rows = []
    if "claude" in only:
        rows += check_claude(args.claude_bin, wanted, tiers)[0]
    if "agy" in only:
        rows += check_agy(args.agy_bin, tiers)
    if "ollama" in only:
        rows += check_ollama(args.ollama)
    if "api" in only:
        rows += check_api(os.environ)
    # What the config names but no catalogue lists: a configured model the account does not offer.
    listed = {r["model"] for r in rows}
    for m, t in tiers.items():
        if m not in listed and not any(x.startswith("claude:") for x in t):
            rows.append(row("config", m, "unlisted", "configured for %s but no catalogue lists it" % ", ".join(t), tiers=t, source="offline"))
    apply_exhaustion(rows, offline_state(ws), time.time())
    if args.live:
        run_live(rows, args)
    return {"generated_at": iso(time.time()), "home_workspace": args.workspace, "live": bool(args.live), "models": rows}


def render(report):
    order = {"available": 0, "unknown": 1, "exhausted": 2, "unlisted": 3, "unauthenticated": 4, "unreachable": 5, "error": 6, "skipped": 7}
    rows = sorted(report["models"], key=lambda r: (r["provider"], order.get(r["status"], 9), r["model"]))
    w = max([len(r["model"]) for r in rows] + [5])
    out = ["%-12s %-*s %-32s %-24s %s" % ("provider", w, "model", "status", "tiers", "detail")]
    for r in rows:
        status = r["status"] + (" until " + iso(r["until"]) if r["status"] == "exhausted" and r.get("until") else "")
        out.append("%-12s %-*s %-32s %-24s %s" % (r["provider"], w, r["model"], status, ",".join(r["tiers"])[:24], (("[%s] " % r["source"]) if r["source"] else "") + r["detail"]))
    counts = {}
    for r in rows:
        counts[r["status"]] = counts.get(r["status"], 0) + 1
    out.append("")
    out.append("%s | %s" % (report["generated_at"], ", ".join("%d %s" % (n, s) for s, n in sorted(counts.items()))) + ("" if report["live"] else " | not called: add --live to learn quota (a few thousand cached tokens per model)"))
    for r in rows:
        if r["provider"] == "claude" and r.get("windows"):
            out.append("claude subscription: " + ", ".join("%s %.0f%% used, resets %s" % (k, 100 * (v.get("used") or 0), iso(v["resets_at"]) if v.get("resets_at") else "?") for k, v in sorted(r["windows"].items())))
            break
    return "\n".join(out)


# ── self-test ───────────────────────────────────────────────────────────────────────────────────────────────────

def self_test():
    assert duration_secs("101h32m27s") == 101 * 3600 + 32 * 60 + 27
    assert duration_secs("3h") == 10800 and duration_secs("45m") == 2700 and duration_secs("90s") == 90 and duration_secs("2d4h") == 2 * 86400 + 4 * 3600
    assert duration_secs("soon") is None and duration_secs("") is None

    cfg = parse_config('''
# a comment
[models.claude]
fast = "haiku"
workhorse = "sonnet"
frontier = "opus"   # trailing
[models.gemini]
fast = "<gemini fast model id>"
[[endpoints]]
id = "claude-sub"
family = "claude"
path = "harness"
credential = "harness-login"
[[endpoints]]
id = "ollama"
family = "local"
base_url = "http://localhost:11434"
models = { fast = "qwen3:8b", workhorse = "qwen3:8b" }
''')
    assert cfg["tiers"]["claude"] == {"fast": "haiku", "workhorse": "sonnet", "frontier": "opus"}
    assert cfg["tiers"]["gemini"] == {}, "placeholder ids are not models"
    assert [e["id"] for e in cfg["endpoints"]] == ["claude-sub", "ollama"] and cfg["endpoints"][1]["models"] == {"fast": "qwen3:8b", "workhorse": "qwen3:8b"}
    t = tiers_of(cfg)
    assert t["haiku"] == ["claude:fast"] and sorted(t["qwen3:8b"]) == ["local:fast", "local:workhorse"]

    assert parse_agy_models("Fetching available models...\ngemini-3.8-flash-low\tGemini 3.8 Flash (Low)\nclaude-sonnet-5-5-low\tClaude Sonnet 5.5 (Low)\n") == [("gemini-3.8-flash-low", "Gemini 3.8 Flash (Low)"), ("claude-sonnet-5-5-low", "Claude Sonnet 5.5 (Low)")]

    ok = "\n".join([json.dumps({"type": "system", "subtype": "init"}),
                    json.dumps({"type": "rate_limit_event", "rate_limit_info": {"status": "allowed", "unifiedWindows": {"five_hour": {"utilization": 0.84, "resetsAt": 1793032200, "status": "allowed"}, "seven_day": {"utilization": 0.56, "resetsAt": 1793365200, "status": "allowed"}}}}),
                    json.dumps({"type": "result", "is_error": False, "result": "OK", "usage": {"input_tokens": 2, "cache_creation_input_tokens": 100, "cache_read_input_tokens": 50, "output_tokens": 4}})])
    s, d, until, win, toks = classify_claude_stream(ok)
    assert (s, until, toks) == ("available", None, 156) and win["five_hour"]["used"] == 0.84 and win["seven_day"]["resets_at"] == 1793365200
    rej = json.dumps({"type": "rate_limit_event", "rate_limit_info": {"status": "rejected", "rateLimitType": "five_hour", "resetsAt": 1792864800, "unifiedWindows": {"five_hour": {"utilization": 1, "resetsAt": 1792864800, "status": "rejected"}}}})
    s, d, until, win, _ = classify_claude_stream(rej)
    assert s == "exhausted" and until == 1792864800 and "2026" in d
    limited = json.dumps({"type": "result", "is_error": True, "result": "Claude AI usage limit reached|1792864800"})
    s, d, until, _, _ = classify_claude_stream(limited)
    assert (s, until) == ("exhausted", 1792864800)
    s, d, _, _, _ = classify_claude_stream(json.dumps({"type": "result", "is_error": True, "result": "There's an issue with the selected model (nope). It may not exist"}))
    assert s == "unlisted"
    assert classify_claude_stream("garbage")[0] == "error"

    good = json.dumps({"event": "result", "result": {"status": "SUCCESS", "response": "OK", "usage": {"total_tokens": 6780}}})
    assert classify_agy_stream(good) == ("available", "", None, 6780)
    quota = "{\"event\":\"init\"}\nerror: Individual quota reached. Please upgrade your subscription to increase your limits. Resets in 101h32m27s.\nAGY_ERROR: {\"short_error\":\"RESOURCE_EXHAUSTED (code 429)\"}"
    s, d, until, _ = classify_agy_stream(quota, now=1000)
    assert s == "exhausted" and until == 1000 + 101 * 3600 + 32 * 60 + 27 and d.startswith("error: Individual quota")
    assert classify_agy_stream("error: model foo not found")[0] == "unlisted"
    assert classify_agy_stream("please sign in")[0] == "unauthenticated"
    assert classify_agy_stream("")[0] == "error"

    # Exhaustion from the saved pool applies only to unchecked models and only until its time.
    rows = [row("agy", "m1", "unknown"), row("agy", "m2", "unknown"), row("agy", "m3", "available")]
    ex = {("agy-sub", "m1"): {"until": 2000, "reason": "quota reached"}, ("agy-sub", "m3"): {"until": 2000}}
    apply_exhaustion(rows, ex, 1000)
    assert [r["status"] for r in rows] == ["exhausted", "unknown", "available"] and rows[0]["source"] == "offline"
    rows2 = [row("agy", "m1", "unknown")]
    apply_exhaustion(rows2, ex, 3000)
    assert rows2[0]["status"] == "unknown", "a reset time in the past is no longer exhaustion"

    # The whole thing against fake CLIs: no network, no model.
    d = pathlib.Path(tempfile.mkdtemp(prefix="avail-test-"))
    try:
        claude = d / "claude"
        claude.write_text("#!/bin/sh\ncase \"$1\" in --version) echo '9.9.9 (Claude Code)';; auth) echo '{\"loggedIn\": true, \"authMethod\": \"claude.ai\", \"apiProvider\": \"firstParty\"}';; *) cat <<'EOF'\n" + ok + "\nEOF\n;; esac\n")
        agy = d / "agy"
        agy.write_text("#!/bin/sh\ncase \"$1\" in --version) echo 1.4.7;; models) printf 'Fetching...\\ngood-model\\tGood\\nbad-model\\tBad\\n';; --print) case \"$*\" in *bad-model*) echo 'error: Individual quota reached. Resets in 2h.';; *) cat <<'EOF'\n" + good + "\nEOF\n;; esac;; esac\n")
        for p in (claude, agy):
            p.chmod(0o755)
        home = d / "home"
        (home / "workspaces" / "w" / "local").mkdir(parents=True)
        (home / "workspaces" / "w" / "config.toml").write_text('[models.claude]\nfast = "haiku"\nworkhorse = "sonnet"\n')
        (home / "workspaces" / "w" / "local" / "pool-state.json").write_text(json.dumps({"entries": [{"endpoint_id": "agy-sub", "model_id": "bad-model", "state": "exhausted", "until": int(time.time()) + 3600, "reason": "quota reached"}]}))
        ns = argparse.Namespace(home=str(home), workspace="w", live=False, only="claude,agy", claude_models="haiku,sonnet", claude_bin=str(claude), agy_bin=str(agy), ollama="http://127.0.0.1:1", json=None)
        rep = survey(ns)
        by = {(r["provider"], r["model"]): r for r in rep["models"]}
        assert by[("claude", "haiku")]["status"] == "unknown" and "signed in via claude.ai" in by[("claude", "haiku")]["detail"]
        assert by[("agy", "good-model")]["status"] == "unknown" and by[("agy", "bad-model")]["status"] == "exhausted" and by[("agy", "bad-model")]["source"] == "offline"
        assert by[("claude", "haiku")]["tiers"] == ["claude:fast"]
        ns.live = True
        rep = survey(ns)
        by = {(r["provider"], r["model"]): r for r in rep["models"]}
        assert by[("claude", "haiku")]["status"] == "available" and by[("claude", "haiku")]["windows"]["five_hour"]["used"] == 0.84
        assert by[("agy", "good-model")]["status"] == "available" and by[("agy", "bad-model")]["status"] == "exhausted" and by[("agy", "bad-model")]["source"] == "live"
        text = render(rep)
        assert "claude subscription: five_hour 84% used" in text and "exhausted until" in text
        ns2 = argparse.Namespace(**{**vars(ns), "claude_bin": str(d / "nope")})
        assert all(r["status"] == "unreachable" for r in survey(ns2)["models"] if r["provider"] == "claude")
    finally:
        shutil.rmtree(d, ignore_errors=True)
    print("self-test ok")


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("cmd", nargs="?", default="survey", choices=["survey", "self-test"])
    ap.add_argument("--home", default=os.environ.get("DELPHI_HOME", "~/.delphi"))
    ap.add_argument("--workspace", default=os.environ.get("DELPHI_WORKSPACE", "default"))
    ap.add_argument("--live", action="store_true", help="one tiny call per model, to learn about quota")
    ap.add_argument("--json", help="write the full report here")
    ap.add_argument("--claude-models", default=",".join(CLAUDE_DEFAULTS))
    ap.add_argument("--only", help="comma list of claude,agy,ollama,api")
    ap.add_argument("--claude-bin", default="claude")
    ap.add_argument("--agy-bin", default="agy")
    ap.add_argument("--ollama", default="http://localhost:11434")
    args = ap.parse_args(argv)
    if args.cmd == "self-test":
        self_test()
        return 0
    rep = survey(args)
    print(render(rep))
    if args.json:
        pathlib.Path(args.json).write_text(json.dumps(rep, indent=2) + "\n")
        print("wrote", args.json)
    return 0


if __name__ == "__main__":
    sys.exit(main())
