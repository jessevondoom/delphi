#!/bin/sh
# Spec 110 X4 (and spec 105 H4): a live check of Delphi in Claude Code. Usage:
#   dev/hosts/claude-code.sh            probe: Delphi's server by --mcp-config, probe hooks and a worker file in the scratch project
#   DELPHI_PROBE_MODE=final dev/hosts/claude-code.sh
#                                       final: the generated plugin by --plugin-dir, [mcp] session_hooks on in the temporary home
#
# LIMIT (spec 110 A1): the Delphi server and hooks run against a temporary Delphi home; Claude Code itself runs as the operator
# with the operator's sign-in. It is configured only by flags: --strict-mcp-config --mcp-config <scratch file>, --setting-sources
# project (the scratch project's settings only, so the operator's own hooks and plugins are not loaded), --plugin-dir, and
# --permission-prompts none (anything that would ask is denied; no permission is bypassed). Three model calls, model haiku.
# The final run keeps its session transcript (spec 105 H3 reads it); the probe run uses --no-session-persistence.

. "$(dirname "$0")/lib.sh"
command -v claude >/dev/null || die "claude is not installed"
setup_scratch claude-code
MODEL=${CLAUDE_PROBE_MODEL:-haiku}

cat >"$S/mcp.json" <<EOF
{ "mcpServers": { "delphi": { "command": "$S/bin/delphi", "args": ["mcp"] } } }
EOF
write_instruction_canary CLAUDE.md
TOOLS=""
for p in mcp__delphi__ mcp__plugin_delphi_delphi__; do
    for t in list_roles get_agent get_skill list_skills search_memory kb_retrieve search_kb specs_status specs_show specs_list search_project query_memory consult_orchestrator; do
        TOOLS="$TOOLS ${p}$t"
    done
done
EXTRA=""
if [ "$MODE" = final ]; then
    hooks_on_in_temp_home
    EXTRA="--plugin-dir $REPO/ecosystem/claude-code/delphi"
    WORKER=delphi:architect-worker
    PERSIST=""
else
    mkdir -p "$P/.claude/agents"
    cp "$REPO/ecosystem/claude-code/delphi/agents/architect-worker.md" "$P/.claude/agents/"
    cat >"$P/.claude/settings.json" <<EOF
{ "hooks": {
  "SessionStart": [ { "hooks": [ { "type": "command", "command": "$S/bin/probe-hook session-start" } ] } ],
  "UserPromptSubmit": [ { "hooks": [ { "type": "command", "command": "$S/bin/probe-hook prompt" } ] } ],
  "SessionEnd": [ { "hooks": [ { "type": "command", "command": "$S/bin/probe-hook session-end" } ] } ]
} }
EOF
    WORKER=architect-worker
    PERSIST="--no-session-persistence"
fi

run() {
    n=$1
    shift
    model_call
    # shellcheck disable=SC2086
    (cd "$P" && claude -p "$1" --model "$MODEL" --strict-mcp-config --mcp-config "$S/mcp.json" --setting-sources project \
        --permission-prompts none --allowedTools $TOOLS Read Grep Glob Agent Task $EXTRA $PERSIST --output-format json \
        >"$LOG/reply-$n.json" 2>"$LOG/reply-$n.err" </dev/null) || true
    collect "$n"
    printf '    claude -p <prompt %s> --model %s --strict-mcp-config --mcp-config <scratch>/mcp.json --setting-sources project --permission-prompts none --allowedTools <the Delphi read tools> Read Grep Glob Agent Task %s %s --output-format json\n' \
        "$n" "$MODEL" "$(echo "$EXTRA" | sed "s#$REPO/##")" "$PERSIST" >>"$LOG/commands.txt"
}

run 1 "$PROMPT_TOOLS"
run 2 "$PROMPT_MENTION"
run 3 "$(prompt_worker "$WORKER")"

summarize "Claude Code" "$(claude --version 2>/dev/null | head -1)" "$WORKER"

