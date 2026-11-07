"""Host conformance probe for spec 086 section 3 (slice S0b).

Run with `python3 -I dev/host-probe/host_probe.py ...`.

  render  [--results FILE] [--out FILE] [--check]

Default is a DRY RUN: it records each installed CLI's version (`<bin> --version`)
and writes the canary agent files into a temp directory. It spends NO model
tokens. Only `--live` asks a host to spawn two seats and call the Delphi MCP
tool `list_roles` from inside a seat; that spends the operator's tokens, so
only the operator runs it. Nothing here widens authority: the probe never
approves, installs or configures anything on a host.

Cell values: verified | failed | not run. A dry run never sets a cell to
verified or failed except by what it observed (it observes none of them).
"""
import argparse
import datetime
import json
import os
import re
import shlex
import shutil
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
DEFAULT_RESULTS = os.path.join(HERE, "results.json")
DEFAULT_DOC = os.path.join(REPO, "docs", "reference", "host-subagents.md")

HOSTS = ["claude-code", "copilot", "codex", "agy"]
HOST_TITLES = {
    "claude-code": "Claude Code",
    "copilot": "Copilot CLI",
    "codex": "Codex CLI",
    "agy": "agy (Antigravity CLI)",
}
BIN = {"claude-code": "claude", "copilot": "copilot", "codex": "codex", "agy": "agy"}

# Spec 086 section 3 rows, in table order. `live` marks the cells the --live probe can set.
CELLS = [
    ("definition", "Subagent definition", False),
    ("plugin", "Ships in a plugin", False),
    ("location", "Project / user location", False),
    ("isolated", "Isolated context", False),
    ("parallel", "Parallel seats", True),
    ("model_pin", "Pin a model per agent", True),
    ("mcp", "MCP in a seat", True),
    ("tools", "Tool allowlist", False),
    ("nesting", "Nesting limit", False),
    ("slash", "Slash entry", False),
]
# Plus one cell the probe sets that is not a table row: the canary file was found.
EXTRA_CELLS = [("discovered", "Agent discovered", True)]
STATUSES = ("verified", "failed", "not run")

# Vendor-doc claims, spec 086 section 3 (read 2026-11-05; not live-proven).
CLAIMS = {
    "claude-code": {
        "definition": "`agents/<name>.md`",
        "plugin": "yes, `agents/`; names are scoped `delphi:architect`",
        "location": "`.claude/agents/`, `~/.claude/agents/`",
        "isolated": "yes",
        "parallel": "yes, default cap 20",
        "model_pin": "`model:` alias (`sonnet`, `opus`, `haiku`, `fable`), full id, or `inherit`",
        "mcp": "inherits the parent's MCP tools; a plugin agent may not set `mcpServers` (ignored), so the server is declared at plugin level",
        "tools": "`tools`, `disallowedTools`",
        "nesting": "3 by default",
        "slash": "plugin `commands/` or skill",
        "discovered": "the canary file under `.claude/agents/` is listed",
    },
    "copilot": {
        "definition": "`<id>.agent.md`",
        "plugin": "yes in legacy plugins, `agents/`; \"Agent Plugins 1.0\" does not define portable agents",
        "location": "`.github/agents/`, `~/.copilot/agents/`",
        "isolated": "yes (own context window)",
        "parallel": "docs silent (blog says the main agent can run them in parallel)",
        "model_pin": "`model:` (+ `model-policy`)",
        "mcp": "`mcp-servers:` in a plugin agent's frontmatter; inheritance docs silent",
        "tools": "not documented as a standalone field",
        "nesting": "n/a",
        "slash": "plugin `commands` field, or skill",
        "discovered": "the canary file under `.github/agents/` is listed",
    },
    "codex": {
        "definition": "`<name>.toml`",
        "plugin": "docs silent",
        "location": "`.codex/agents/`, `~/.codex/agents/`",
        "isolated": "yes",
        "parallel": "yes, but only when the prompt asks (\"one agent per seat\"); `agents.max_concurrent_threads_per_session`",
        "model_pin": "`model` and `model_reasoning_effort`",
        "mcp": "inherits the parent's MCP servers; the file cannot set `mcp_servers`",
        "tools": "feature switches only; no sandbox override",
        "nesting": "not specified",
        "slash": "skill",
        "discovered": "the canary file under `.codex/agents/` is listed",
    },
    "agy": {
        "definition": "`<name>.md` or `<name>/agent.md`",
        "plugin": "yes, `plugins/<plugin>/agents/`",
        "location": "`.agents/agents/`, `~/.gemini/config/agents/`",
        "isolated": "yes (no parent history)",
        "parallel": "yes, `invoke_subagent`, concurrent",
        "model_pin": "`model:` `inherit`, `flash` or `pro` (tiers, not versions)",
        "mcp": "`mcpServers:` per agent is the only documented route; inheritance docs silent",
        "tools": "`tools` (a wrong name may hang the subagent)",
        "nesting": "10",
        "slash": "skills load with `/<skill-name>`",
        "discovered": "the canary file under `.agents/agents/` is listed",
    },
}

# Default model pin written into the canary (a cheap tier). Codex has no
# documented cheap alias, so it is only pinned when the operator passes --pin.
DEFAULT_PIN = {"claude-code": "haiku", "copilot": None, "codex": None, "agy": "flash"}

# Non-interactive invocation templates. UNVERIFIED: the first live run by the
# operator decides; override with --cmd HOST='...{prompt}...'.
LIVE_CMD = {
    "claude-code": "claude -p {prompt}",
    "copilot": "copilot -p {prompt} --allow-all-tools",
    # codex 0.166.0 refuses to run outside a trusted directory ("Not inside a trusted directory and --skip-git-repo-check was not
    # specified"); the probe runs in a temporary directory that holds only its two canary files, so the check is skipped here only.
    "codex": "codex exec --skip-git-repo-check {prompt}",
    "agy": "agy -p {prompt}",
}

SEATS = ["delphi-canary-a", "delphi-canary-b"]
CANARY_BODY = (
    "You are a conformance canary seat, nothing more. Do exactly this and "
    "print each line verbatim, with no other prose.\n"
    "1. Run the shell command `date +%s` and print `SEAT {name} START <output>`.\n"
    "2. Run `sleep 8`.\n"
    "3. Call the MCP tool `list_roles` (Delphi). If you can call it, print "
    "`SEAT {name} MCP direct <number of roles>`; if no such tool is visible to "
    "you, print `SEAT {name} MCP none 0`.\n"
    "4. Run `date +%s` and print `SEAT {name} END <output>`.\n"
    "5. Print `SEAT {name} CANARY-OK`.\n"
)
DESCRIPTION = "Delphi conformance canary. Use only when asked to run the canary seats."


def canary_files(host, pin):
    """Return {relative path: text} for the two canary agents in `host`'s format (spec 086 section 3)."""
    out = {}
    for name in SEATS:
        body = CANARY_BODY.format(name=name)
        if host == "claude-code":
            fm = ["---", f"name: {name}", f"description: {DESCRIPTION}"]
            if pin:
                fm.append(f"model: {pin}")
            out[f".claude/agents/{name}.md"] = "\n".join(fm + ["---", "", body])
        elif host == "copilot":
            fm = ["---", f"name: {name}", f"description: {DESCRIPTION}"]
            if pin:
                fm.append(f"model: {pin}")
            out[f".github/agents/{name}.agent.md"] = "\n".join(fm + ["---", "", body])
        elif host == "codex":
            lines = [f'name = "{name}"', f'description = "{DESCRIPTION}"']
            if pin:
                lines.append(f'model = "{pin}"')
            lines.append('developer_instructions = """\n' + body + '"""')
            out[f".codex/agents/{name}.toml"] = "\n".join(lines) + "\n"
        elif host == "agy":
            fm = ["---", f"name: {name}", f"description: {DESCRIPTION}"]
            # agy gives a subagent no tools unless its frontmatter declares them (docs: `tools` defaults to empty); the seats run
            # `date +%s` and `sleep 8`, and headless agy also needs a matching `command(...)` allow rule in its settings.json.
            fm.append("tools: [run_command]")
            if pin:
                fm.append(f"model: {pin}")
            out[f".agents/agents/{name}.md"] = "\n".join(fm + ["---", "", body])
        else:
            raise ValueError(host)
    return out


def live_prompt(host):
    spawn = (
        "Spawn one agent per seat, in parallel, wait for all of them, and return each "
        "seat's lines verbatim. "
    )
    return (
        f"Run the conformance canary. {spawn}The two seats are the agents named "
        f"{SEATS[0]} and {SEATS[1]}. Each is an isolated subagent defined in this "
        "project's agents directory. Do not answer for them and do not paraphrase."
    )


def now_iso():
    return datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%d")


def blank_entry(host):
    cells = {k: {"status": "not run"} for k, _, _ in CELLS + EXTRA_CELLS}
    return {"host": host, "cli_version": None, "date": None, "cells": cells}


def load_results(path):
    if os.path.exists(path):
        with open(path, encoding="utf-8") as f:
            data = json.load(f)
    else:
        data = []
    by = {e["host"]: e for e in data}
    return [by.get(h) or blank_entry(h) for h in HOSTS]


def save_results(path, entries):
    with open(path, "w", encoding="utf-8") as f:
        json.dump(entries, f, indent=2)
        f.write("\n")


def cli_version(host, timeout=20):
    exe = shutil.which(BIN[host])
    if not exe:
        return None, "not installed"
    try:
        p = subprocess.run([exe, "--version"], capture_output=True, text=True, timeout=timeout)
    except (OSError, subprocess.TimeoutExpired) as e:
        return None, f"--version failed: {e}"
    text = (p.stdout or p.stderr).strip().splitlines()
    return (text[0] if text else None), ("" if p.returncode == 0 else f"--version exit {p.returncode}")


def write_canaries(root, host, pin):
    paths = []
    for rel, text in canary_files(host, pin).items():
        full = os.path.join(root, rel)
        os.makedirs(os.path.dirname(full), exist_ok=True)
        with open(full, "w", encoding="utf-8") as f:
            f.write(text)
        paths.append(rel)
    return paths


def analyse(output, exit_code, pin):
    """Turn a live run's output into cell results. Pure; unit-tested."""
    cells = {}
    seen = [n for n in SEATS if f"SEAT {n} CANARY-OK" in output]
    if len(seen) == len(SEATS):
        cells["discovered"] = {"status": "verified", "note": "both canary seats ran"}
    elif exit_code != 0 or not seen:
        cells["discovered"] = {"status": "failed", "note": f"{len(seen)} of 2 seats reported (exit {exit_code})"}
    else:
        cells["discovered"] = {"status": "failed", "note": f"only {seen[0]} reported"}

    def stamp(n, kind):
        m = re.search(rf"SEAT {re.escape(n)} {kind} (\d+)", output)
        return int(m.group(1)) if m else None

    iv = [(stamp(n, "START"), stamp(n, "END")) for n in SEATS]
    if all(a is not None and b is not None for a, b in iv):
        overlap = min(iv[0][1], iv[1][1]) - max(iv[0][0], iv[1][0])
        if overlap > 0:
            cells["parallel"] = {"status": "verified", "note": f"seats overlapped by {overlap}s"}
        else:
            cells["parallel"] = {"status": "failed", "note": "seats ran one after the other"}
    else:
        cells["parallel"] = {"status": "failed", "note": "no timestamps from both seats"}

    mcp = [re.search(rf"SEAT {re.escape(n)} MCP (direct|none) (\d+)", output) for n in SEATS]
    got = [m for m in mcp if m]
    if any(m.group(1) == "direct" and int(m.group(2)) > 0 for m in got):
        cells["mcp"] = {"status": "verified", "note": "direct: a seat called list_roles itself"}
    elif got:
        cells["mcp"] = {"status": "verified", "note": "relayed: the seat saw no MCP tools"}
    else:
        cells["mcp"] = {"status": "failed", "note": "no seat reported on MCP"}

    if pin is None:
        cells["model_pin"] = {"status": "not run", "note": "no pin written; pass --pin HOST=MODEL"}
    elif cells["discovered"]["status"] == "verified":
        cells["model_pin"] = {"status": "verified", "note": f"seats ran with model: {pin} (accepted, not proof it was used)"}
    elif re.search(r"model", output, re.I) and exit_code != 0:
        cells["model_pin"] = {"status": "failed", "note": f"host rejected model: {pin}"}
    else:
        cells["model_pin"] = {"status": "not run", "note": "seats did not run, so the pin is untested"}
    return cells


def run_live(host, root, cmd_template, timeout):
    cmd = shlex.split(cmd_template.replace("{prompt}", shlex.quote(live_prompt(host))))
    p = subprocess.run(cmd, cwd=root, capture_output=True, text=True, timeout=timeout)
    return (p.stdout or "") + "\n" + (p.stderr or ""), p.returncode


def cmd_probe(a):
    entries = load_results(a.results)
    by = {e["host"]: e for e in entries}
    cmds = dict(LIVE_CMD)
    for item in a.cmd or []:
        h, _, t = item.partition("=")
        cmds[h] = t
    pins = dict(DEFAULT_PIN)
    for item in a.pin or []:
        h, _, m = item.partition("=")
        pins[h] = m or None
    if a.live:
        print("LIVE: this spends the operator's model tokens on each host probed.")
    else:
        print("DRY RUN: versions and canary files only. NO model tokens are spent. Pass --live to probe seats.")
    for host in a.host or HOSTS:
        if host not in HOSTS:
            sys.exit(f"unknown host {host}; one of {', '.join(HOSTS)}")
        e = by[host]
        ver, note = cli_version(host)
        root = tempfile.mkdtemp(prefix=f"delphi-host-probe-{host}-")
        paths = write_canaries(root, host, pins[host])
        print(f"{host}: version={ver or 'unknown'}{' (' + note + ')' if note else ''}")
        print(f"  canary files in {root}: {', '.join(paths)}")
        if ver:
            e["cli_version"] = ver
            e["date"] = now_iso()
        if a.live:
            if not ver:
                print("  skipped: CLI not installed")
            else:
                try:
                    out, code = run_live(host, root, cmds[host], a.timeout)
                except subprocess.TimeoutExpired:
                    out, code = "", 124
                for k, v in analyse(out, code, pins[host]).items():
                    e["cells"][k] = v
                    print(f"  {k}: {v['status']} ({v.get('note', '')})")
    if a.live or a.write:
        save_results(a.results, entries)
        print(f"results written to {a.results}")
    else:
        print("dry run: results file not changed (use --write to record versions)")


def fmt_cell(entry, key):
    c = entry["cells"].get(key, {"status": "not run"})
    s = c["status"]
    if s not in STATUSES:
        raise ValueError(f"bad status {s!r}")
    return s + (f" ({c['note']})" if c.get("note") else "")


def render(entries):
    out = [
        "# Host subagents conformance",
        "",
        "Generated by `python3 -I dev/host-probe/host_probe.py render` from `dev/host-probe/results.json`; do not edit by hand. "
        "Spec 086 section 3 (slice S0b). Each cell shows the result of the conformance probe (`verified`, `failed` or `not run`) "
        "and, beside it, the vendor-docs claim it checks (read 2026-11-05; not live-proven). "
        "`not run` means nobody has run `--live` for that host: the claim is unproven. "
        "To run it: `python3 -I dev/host-probe/host_probe.py probe --live --host <host>` (spends your tokens).",
        "",
    ]
    for e in entries:
        h = e["host"]
        out += [
            f"## {HOST_TITLES[h]}",
            "",
            f"CLI version: {e['cli_version'] or 'not recorded'}. Probed: {e['date'] or 'never'}.",
            "",
            "| Cell | Result | Vendor-docs claim |",
            "| :-- | :-- | :-- |",
        ]
        for key, title, _ in EXTRA_CELLS + CELLS:
            out.append(f"| {title} | {fmt_cell(e, key)} | {CLAIMS[h][key]} |")
        out.append("")
    return "\n".join(out)


def cmd_render(a):
    text = render(load_results(a.results))
    if a.check:
        have = open(a.out, encoding="utf-8").read() if os.path.exists(a.out) else ""
        if have != text:
            sys.exit(f"{a.out} is out of date; run: python3 -I dev/host-probe/host_probe.py render")
        print("up to date")
        return
    with open(a.out, "w", encoding="utf-8") as f:
        f.write(text)
    print(f"wrote {a.out}")


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("probe")
    p.add_argument("--host", action="append")
    p.add_argument("--live", action="store_true", help="spend tokens: spawn two seats on each host")
    p.add_argument("--write", action="store_true", help="record versions in the results file on a dry run")
    p.add_argument("--pin", action="append", metavar="HOST=MODEL")
    p.add_argument("--cmd", action="append", metavar="HOST=TEMPLATE")
    p.add_argument("--timeout", type=int, default=600)
    p.add_argument("--results", default=DEFAULT_RESULTS)
    p.set_defaults(fn=cmd_probe)
    r = sub.add_parser("render")
    r.add_argument("--results", default=DEFAULT_RESULTS)
    r.add_argument("--out", default=DEFAULT_DOC)
    r.add_argument("--check", action="store_true")
    r.set_defaults(fn=cmd_render)
    a = ap.parse_args(argv)
    a.fn(a)


if __name__ == "__main__":
    main()
