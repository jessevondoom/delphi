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
if [ "$MODE" = final ]; then
    # The instruction section `delphi connect codex` puts in Codex's AGENTS.md, here in the scratch project's AGENTS.md instead.
    cat "$S/export/DELPHI.md" >>"$P/AGENTS.md"
fi

run() {
    n=$1
    shift
    model_call
    (cd "$P" && codex exec --ephemeral --ignore-user-config --skip-git-repo-check -s read-only -C "$P" -m "$MODEL" \
        -c 'model_reasoning_effort="low"' -c "mcp_servers.delphi.command=\"$S/bin/delphi\"" -c 'mcp_servers.delphi.args=["mcp"]' \
        --json "$1" >"$LOG/reply-$n.json" 2>"$LOG/reply-$n.err" </dev/null) || true
    collect "$n"
    printf '    codex exec --ephemeral --ignore-user-config --skip-git-repo-check -s read-only -C <scratch>/proj_x.y -m %s -c model_reasoning_effort="low" -c mcp_servers.delphi.command="<scratch>/bin/delphi" -c mcp_servers.delphi.args=["mcp"] --json <prompt %s>\n' \
        "$MODEL" "$n" >>"$LOG/commands.txt"
}

run 1 "$PROMPT_TOOLS"
run 2 "$PROMPT_MENTION"
run 3 "$(prompt_worker "$WORKER")"

summarize "Codex CLI" "$(codex --version 2>/dev/null | head -1)" "$WORKER"
row "worker files written" "$(ls "$P/.codex/agents" 2>/dev/null | wc -l | tr -d ' ') under the scratch project's .codex/agents"
