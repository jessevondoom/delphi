#!/bin/sh
# Spec 110 X4: a live check of Delphi in Codex CLI. Usage:
#   dev/hosts/codex.sh                         probe: server by -c overrides, a worker file in the scratch project's .codex/agents
#   DELPHI_PROBE_MODE=final dev/hosts/codex.sh final: the same, with the instruction section `delphi connect codex` writes,
#                                              placed in the scratch project's AGENTS.md (never Codex's own)
#
# LIMIT (spec 110 A1): the Delphi server runs against a temporary Delphi home; Codex itself runs as the operator with the
# operator's sign-in (CODEX_HOME is the operator's: Codex reads its credentials there and a scratch copy would mean copying
# them, which this script never does). It is configured only by flags: --ignore-user-config (the operator's config.toml is not
# loaded), -c mcp_servers.delphi.* (the server, for this run only), --ephemeral (no session files), -s read-only (Codex's
# read-only sandbox), --skip-git-repo-check. Nothing is trusted: no --trust, no hook trust, no bypass flag. Three model calls.
# Codex runs hooks only once their trust is persisted in its config, so hooks are not testable here.

. "$(dirname "$0")/lib.sh"
command -v codex >/dev/null || die "codex is not installed"
setup_scratch codex
MODEL=${CODEX_PROBE_MODEL:-gpt-5.6-luna}

write_instruction_canary AGENTS.md
WORKER=delphi-architect-worker
"$DELPHI_BIN" plugin export --host codex --out "$S/export" >/dev/null
mkdir -p "$P/.codex/agents"
(cd "$P" && "$DELPHI_BIN" mcp --install codex --agents --project --yes --home "$H" >"$LOG/install-agents.txt" 2>&1 </dev/null) || true
if [ "$MODE" = final ] || [ "$MODE" = g3 ]; then
    # The instruction section `delphi connect codex` puts in Codex's AGENTS.md, here in the scratch project's AGENTS.md instead.
    cat "$S/export/DELPHI.md" >>"$P/AGENTS.md"
fi
# Spec 110 §10 G3b, the agent-format probe: Codex 0.169.0's own config keys (`[agents.<name>]` with `description` and
# `config_file`, from its binary's config schema; a role file needs `name`, `description` and `developer_instructions`)
# declare the generated worker for this run only, by -c flags, with the role file in the scratch directory.
ROLES=""
if [ "$MODE" = g3 ]; then
    mkdir -p "$S/codex-roles"
    cp "$P/.codex/agents/$WORKER.toml" "$S/codex-roles/" 2>/dev/null || die "the worker file was not written; see $LOG/install-agents.txt"
fi

run() {
    n=$1
    prompt=$2
    model_call
    set --
    if [ -n "$ROLES" ]; then
        set -- -c "agents.$WORKER.description=\"Delphi architect (worker)\"" -c "agents.$WORKER.config_file=\"$S/codex-roles/$WORKER.toml\""
    fi
    (cd "$P" && codex exec --ephemeral --ignore-user-config --skip-git-repo-check -s read-only -C "$P" -m "$MODEL" \
        -c 'model_reasoning_effort="low"' -c "mcp_servers.delphi.command=\"$S/bin/delphi\"" -c 'mcp_servers.delphi.args=["mcp"]' \
        "$@" --json "$prompt" >"$LOG/reply-$n.json" 2>"$LOG/reply-$n.err" </dev/null) || true
    collect "$n"
    printf '    codex exec --ephemeral --ignore-user-config --skip-git-repo-check -s read-only -C <scratch>/proj_x.y -m %s -c model_reasoning_effort="low" -c mcp_servers.delphi.command="<scratch>/bin/delphi" -c mcp_servers.delphi.args=["mcp"] %s--json <prompt %s>\n' \
        "$MODEL" "${ROLES:+-c agents.$WORKER.description=... -c agents.$WORKER.config_file=<scratch>/codex-roles/$WORKER.toml }" "$n" >>"$LOG/commands.txt"
}

if [ "$MODE" = g3 ]; then
    run 1 "$PROMPT_TOOLS"
    run 2 "$PROMPT_SPAWN"
    ROLES=1
    run 3 "$(prompt_worker "$WORKER")"
    summarize_g3 "Codex CLI" "$(codex --version 2>/dev/null | head -1)"
    row "call 2 (spawn prompt): Delphi tools called" "$(report_py r2 calls)"
    row "call 2: the spawned role's first line, as the host reports it" "$(reply_line "$LOG/reply-2.json" "SPAWN:")"
    row "call 2: spawn_agent items in the host's events" "$(grep -o '"agent_type":[^,}]*' "$LOG/reply-2.json" | sort | uniq -c | tr '\n' ' ')"
    row "call 3 (worker declared by -c agents.*): the host's answer" "$(reply_line "$LOG/reply-3.json" "WORKER:")"
    row "call 3: Delphi tools called" "$(report_py r3 calls)"
    row "call 3: agent_type in the host's events" "$(grep -o '"agent_type":[^,}]*' "$LOG/reply-3.json" | sort | uniq -c | tr '\n' ' ')"
    table_tail
    exit 0
fi

run 1 "$PROMPT_TOOLS"
run 2 "$PROMPT_MENTION"
run 3 "$(prompt_worker "$WORKER")"

summarize "Codex CLI" "$(codex --version 2>/dev/null | head -1)" "$WORKER"
row "worker files written" "$(ls "$P/.codex/agents" 2>/dev/null | wc -l | tr -d ' ') under the scratch project's .codex/agents"
