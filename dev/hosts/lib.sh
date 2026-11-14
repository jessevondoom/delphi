# Shared by dev/hosts/<host>.sh (spec 110 X4). Sourced, never run on its own.
#
# LIMIT (spec 110 amendment A1): Delphi is isolated, the host is not. Every Delphi process the host starts runs against a
# temporary Delphi home (DELPHI_HOME, and --home on every command line), so it cannot reach ~/.delphi or a running daemon.
# The host itself still runs as the operator, with the operator's sign-in, and keeps its own session bookkeeping (history,
# logs) in its own home as any run does. Nothing here installs into, adds a server to, or edits the host's configuration:
# the host is configured for the run only by flags, environment variables and files inside the scratch directory.
#
# Every run spends the operator's host tokens. A script makes at most six model calls (one per prompt it sends).

set -eu

HOSTS_DIR=$(cd "$(dirname "$0")" && pwd -P)
REPO=$(cd "$HOSTS_DIR/../.." && pwd -P)
DELPHI_BIN=${DELPHI_BIN:-$REPO/target/debug/delphi}
# probe: today's install, Delphi's files absent (X4a). final: the files `delphi plugin export` and `delphi connect` produce (X4b).
MODE=${DELPHI_PROBE_MODE:-probe}
MAX_CALLS=6
CALLS=0

die() { echo "dev/hosts: $*" >&2; exit 2; }

[ -x "$DELPHI_BIN" ] || die "no Delphi binary at $DELPHI_BIN (run cargo build -p delphi, or set DELPHI_BIN)"

# A scratch directory: the temporary Delphi home, the scratch project, a bin directory with the logging `delphi` wrapper, logs.
setup_scratch() {
    host=$1
    base=${TMPDIR:-/tmp}
    S=$(mktemp -d "$base/delphi-host-$host.XXXXXX")
    S=$(cd "$S" && pwd -P)
    # An underscore and a dot in the project folder test how a host names its transcript folder (spec 105 H4).
    P="$S/proj_x.y"
    H="$S/home"
    LOG="$S/log"
    mkdir -p "$P" "$H" "$LOG" "$S/bin"
    (cd "$P" && git init -q . 2>/dev/null || true)
    (cd "$P" && "$DELPHI_BIN" init --non-interactive --home "$H" >"$LOG/init.txt" 2>&1) || die "delphi init failed; see $LOG/init.txt"
    WS=$(ls -d "$H"/workspaces/*/ | head -1)
    WS=${WS%/}
    write_wrapper
    write_probe_hook
    export DELPHI_HOME="$H"
    export PATH="$S/bin:$PATH"
}

# `delphi` for the host: only `delphi mcp` and `delphi mcp --hook ...` are let through, always with the temporary home, and both
# directions of the stream are logged so the run can read the client's name, the tool list and each tool call.
write_wrapper() {
    cat >"$S/bin/delphi" <<EOF
#!/bin/sh
export DELPHI_HOME='$H'
n=\$\$
if [ "\$#" -eq 1 ] && [ "\$1" = mcp ]; then
    tee '$LOG'/mcp-\$n.in | '$DELPHI_BIN' mcp --home '$H' 2>'$LOG'/mcp-\$n.err | tee '$LOG'/mcp-\$n.out
    exit 0
fi
if [ "\$1" = mcp ] && [ "\${2:-}" = --hook ]; then
    tee '$LOG'/hook-\$3-\$n.in | '$DELPHI_BIN' "\$@" --home '$H' | tee '$LOG'/hook-\$3-\$n.out
    exit 0
fi
echo "dev/hosts wrapper: refused: delphi \$*" >&2
exit 2
EOF
    chmod 755 "$S/bin/delphi"
}

# A hook command for the probe: logs that the event fired (and its stdin) and prints a canary in the shapes hosts read, so the
# prompt can ask whether the hook's output reached the model. Prints nothing a host could act on.
write_probe_hook() {
    cat >"$S/bin/probe-hook" <<EOF
#!/bin/sh
ev=\$1
cat >'$LOG'/probehook-\$ev-\$\$.in
printf '{"hookSpecificOutput":{"hookEventName":"%s","additionalContext":"The session codeword is CANARY-HOOK-%s."},"additionalContext":"The session codeword is CANARY-HOOK-%s."}\n' "\$ev" "\$ev" "\$ev"
exit 0
EOF
    chmod 755 "$S/bin/probe-hook"
}

# Turn `[mcp] session_hooks` on in the TEMPORARY home only (the CLI switch needs a terminal; this home is the script's own).
hooks_on_in_temp_home() {
    cfg="$WS/config.toml"
    if grep -q '^\[mcp\]' "$cfg"; then
        python3 -I - "$cfg" <<'PY'
import sys
p = sys.argv[1]
t = open(p).read().replace("[mcp]\n", "[mcp]\nsession_hooks = true\n", 1)
open(p, "w").write(t)
PY
    else
        printf '\n[mcp]\nsession_hooks = true\n' >>"$cfg"
    fi
}

# The instruction file canary: a fixed word a host only sees if it loads that file into the model's context.
write_instruction_canary() {
    file=$1
    mkdir -p "$(dirname "$P/$file")"
    printf '# Scratch project\n\nThis scratch project'"'"'s codeword is CANARY-INSTR-%s. When you are asked for the project codeword, give it.\n' "$(basename "$file")" >"$P/$file"
}

# One model call. Refuses a seventh.
model_call() {
    CALLS=$((CALLS + 1))
    [ "$CALLS" -le "$MAX_CALLS" ] || die "a run makes at most $MAX_CALLS model calls"
}

PROMPT_TOOLS='This is an automated check of the Delphi MCP server in a scratch folder. Change nothing, run no shell command, edit no file. Do only this: (1) call the Delphi MCP tool list_roles; (2) call the Delphi MCP tool get_agent with role "architect"; (3) then reply with exactly four lines and nothing else: a line starting "TOOLS: " followed by the exact names of every tool you can call in this session (built-in and MCP), comma-separated, as you would call them; a line starting "ADAPTER: " followed by the line of the get_agent result that starts with "- host_adapter:"; a line starting "CANARY: " followed by every project or session codeword you were given (they start with CANARY-), or "none"; a line starting "BRIEF: " followed by "yes" if you were given a Delphi brief (a note that starts "Delphi brief") before this message, else "no".'

PROMPT_MENTION='@architect In one sentence: what makes a spec ready to build? Change nothing, run no shell command, edit no file.'

prompt_worker() {
    printf 'This is an automated check in a scratch folder. Change nothing, run no shell command, edit no file. Delegate this one task to your subagent (custom agent) named "%s": "In one sentence, say which Delphi role you work as." Then reply with one line: "WORKER: yes" and the subagent'"'"'s answer if it ran, or "WORKER: no" and why it could not.' "$1"
}

# ---- Reading the logs ----

# Move what one model call left in the log into its own folder, so each row can say which call it saw.
collect() {
    mkdir -p "$LOG/r$1"
    for f in "$LOG"/mcp-* "$LOG"/hook-* "$LOG"/probehook-*; do
        [ -e "$f" ] && mv "$f" "$LOG/r$1/"
    done
    return 0
}

# report_py <run folder name or "all"> <what> [arg]
report_py() {
    python3 -I - "$LOG" "$@" <<'PY'
import glob, json, os, re, sys
log = sys.argv[1] if sys.argv[2] == "all" else os.path.join(sys.argv[1], sys.argv[2])
sys.argv = sys.argv[:1] + sys.argv[3:]
sys.argv.insert(1, log)
what = sys.argv[2]

def msgs(suffix):
    out = []
    for f in sorted(glob.glob(os.path.join(log, "**", "mcp-*." + suffix), recursive=True)):
        for line in open(f, errors="replace"):
            line = line.strip()
            if not line:
                continue
            try:
                out.append((f, json.loads(line)))
            except ValueError:
                pass
    return out

if what == "client":
    names = sorted({"%s %s" % (m["params"]["clientInfo"].get("name"), m["params"]["clientInfo"].get("version")) for _, m in msgs("in") if m.get("method") == "initialize"})
    print("; ".join(names) or "none")
elif what == "tools":
    counts = sorted({len(m["result"]["tools"]) for _, m in msgs("out") if isinstance(m.get("result"), dict) and "tools" in m["result"]})
    print(",".join(map(str, counts)) or "none")
elif what == "calls":
    calls = [m["params"].get("name") for _, m in msgs("in") if m.get("method") == "tools/call"]
    print(", ".join(calls) or "none")
elif what == "progress":
    tok = [m for _, m in msgs("in") if m.get("method") == "tools/call" and "progressToken" in (m["params"].get("_meta") or {})]
    print("a tools/call carried a progressToken" if tok else "no tools/call carried a progressToken")
elif what == "adapter":
    lines = set()
    for _, m in msgs("out"):
        r = m.get("result")
        if isinstance(r, dict):
            for c in r.get("content") or []:
                for l in (c.get("text") or "").splitlines():
                    if l.startswith("- host_adapter:"):
                        lines.add(l[:160])
    print(" | ".join(sorted(lines)) or "none")
elif what == "hooks":
    fired = sorted({re.sub(r"-\d+\.in$", "", os.path.basename(f)).replace("probehook-", "probe:").replace("hook-", "delphi:") for f in glob.glob(os.path.join(log, "**", "*hook-*.in"), recursive=True)})
    print(", ".join(fired) or "none")
elif what == "hookout":
    outs = []
    for f in sorted(glob.glob(os.path.join(log, "**", "hook-*.out"), recursive=True)):
        t = open(f, errors="replace").read().strip()
        outs.append("%s: %s" % (re.sub(r"-\d+\.out$", "", os.path.basename(f))[5:], "printed %d bytes" % len(t) if t else "printed nothing"))
    print("; ".join(outs) or "none")
elif what == "builtin":
    # The host's own tool names from the model's TOOLS: line, without the Delphi tools (whatever prefix the host gives them).
    ours = set()
    for _, m in msgs("out"):
        r = m.get("result")
        if isinstance(r, dict) and "tools" in r:
            ours |= {x.get("name") for x in r["tools"]}
    line = open(sys.argv[3], errors="replace").read().strip()
    names = [n.strip().strip("`") for n in line.split(",") if n.strip()]
    mine = [n for n in names if not any(n == o or n.endswith("_" + o) or n.endswith("-" + o) or n.endswith("." + o) or n.endswith("/" + o) for o in ours)]
    print("%s (and %d Delphi tools)" % (", ".join(mine), len(names) - len(mine)))
elif what == "hookin":
    # The fields of the first hook input of an event (keys only, and the transcript_path / cwd values), to learn the host's format.
    norm = lambda s: s.replace("-", "").replace("_", "").lower()
    ev = norm(sys.argv[3])
    found = [f for f in glob.glob(os.path.join(log, "**", "*hook-*.in"), recursive=True) if ev in norm(re.sub(r"-\d+\.in$", "", os.path.basename(f)).split("hook-", 1)[1])]
    for f in sorted(found):
        try:
            v = json.loads(open(f).read())
        except ValueError:
            print("not JSON")
            break
        print("keys=%s transcript_path=%s cwd=%s" % (",".join(sorted(v)), v.get("transcript_path") or v.get("transcriptPath"), v.get("cwd")))
        break
    else:
        print("none")
PY
}

# The line of the host's reply that starts with `$2`, from the reply file `$1`.
reply_line() {
    python3 -I - "$1" "$2" <<'PY'
import json, sys
raw = open(sys.argv[1], errors="replace").read()
t = raw
# A JSON reply (Claude Code --output-format json, Gemini -o json): its result text. JSONL (Codex --json, Copilot json): the agent's messages.
try:
    v = json.loads(raw)
    t = v.get("result") or v.get("response") or raw
except ValueError:
    parts = []
    for line in raw.splitlines():
        try:
            e = json.loads(line)
        except ValueError:
            continue
        if not isinstance(e, dict):
            continue
        # Copilot: assistant.message events; Codex: item.completed of an agent_message. Only the agent's words, never the prompt.
        if e.get("type") == "assistant.message":
            parts.append((e.get("data") or {}).get("content") or "")
            continue
        if e.get("type") == "item.completed" and (e.get("item") or {}).get("type") == "agent_message":
            parts.append(e["item"].get("text") or "")
            continue
        if e.get("type", "").startswith(("session.", "user.", "tool.", "model.", "assistant.", "item.", "turn.", "thread.", "commands.", "instruction.")):
            continue
        def texts(x):
            if isinstance(x, dict):
                for k, y in x.items():
                    if k in ("text", "content", "message", "result") and isinstance(y, str):
                        parts.append(y)
                    else:
                        texts(y)
            elif isinstance(x, list):
                for y in x:
                    texts(y)
        texts(e)
    if parts:
        t = "\n".join(parts)
for l in t.splitlines():
    i = l.find(sys.argv[2])
    if i >= 0:
        print(l[i + len(sys.argv[2]):].strip().strip('"').strip()[:600])
        break
else:
    print("(no such line in the reply)")
PY
}

row() { printf '| %s | %s |\n' "$1" "$(printf '%s' "$2" | tr '|' '/')"; }

# The table every host script prints. $1 title, $2 version, $3 the worker's name as the prompt gave it.
summarize() {
    table_head "$1"
    row "host and version" "$2"
    row "server connects; tools listed (count)" "$(report_py all tools)"
    row "client name at initialize" "$(report_py all client)"
    row "call 1: Delphi tools called" "$(report_py r1 calls)"
    row "call 1: get_agent pack's adapter line" "$(report_py r1 adapter)"
    reply_line "$LOG/reply-1.json" "TOOLS:" >"$LOG/tools-line.txt"
    row "call 1: the host's own tool names, as its model lists them" "$(report_py all builtin "$LOG/tools-line.txt")"
    row "call 1: canaries the model saw (instruction file, hook output)" "$(reply_line "$LOG/reply-1.json" "CANARY:")"
    row "call 1: session-start brief seen by the model" "$(reply_line "$LOG/reply-1.json" "BRIEF:")"
    row "hook events that fired (all calls)" "$(report_py all hooks)"
    row "Delphi hook output (all calls)" "$(report_py all hookout)"
    row "session-end hook input" "$(report_py all hookin session-end)"
    row "call 2 (@architect, no tool named): Delphi tools called" "$(report_py r2 calls)"
    row "call 3: worker subagent $3" "$(reply_line "$LOG/reply-3.json" "WORKER:")"
    row "call 3: Delphi tools called" "$(report_py r3 calls)"
    row "progress notification" "$(report_py all progress); not testable: every waiting tool needs the daemon, which a check may not start"
    row "session markers in the temporary home" "$(ls "$WS/local/host-sessions" 2>/dev/null | tr '\n' ' ' || true)"
    row "session-end markers: counts" "$(python3 -I - "$WS/local/host-sessions" <<'PY'
import glob, json, os, sys
out = []
for f in sorted(glob.glob(os.path.join(sys.argv[1], "*.json"))):
    m = json.load(open(f))
    if "prompts" in m:
        out.append("%s: prompts %s, delphi_calls %s, roles %s, transcript read" % (m.get("tool"), m["prompts"], json.dumps(m.get("delphi_calls")), m.get("roles_adopted")))
    else:
        out.append("%s: no counts (transcript not read)" % m.get("tool"))
print("; ".join(out) or "none")
PY
)"
    row "model calls" "$CALLS"
    echo
    echo "Commands (the prompts are the fixed texts in dev/hosts/lib.sh; <scratch> is a new temporary directory):"
    cat "$LOG/commands.txt"
    echo "Scratch directory (logs, replies): $S"
}

table_head() {
    echo
    echo "### $1 ($MODE run, $(date -u +%Y-%m-%d))"
    echo
    echo "| Check | Result |"
    echo "| :-- | :-- |"
}
