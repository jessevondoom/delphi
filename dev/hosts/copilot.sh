#!/bin/sh
# Spec 110 X4: a live check of Delphi in GitHub Copilot CLI. Usage:
#   dev/hosts/copilot.sh                       probe: server by --additional-mcp-config, probe hooks and a worker in the scratch project
#   DELPHI_PROBE_MODE=final dev/hosts/copilot.sh
#                                              final: the files `delphi connect copilot` installs, from a fake home, by --plugin-dir
#
# LIMIT (spec 110 A1): the Delphi server and hooks run against a temporary Delphi home; Copilot CLI itself runs as the operator
# with the operator's sign-in (its own home keeps its session bookkeeping). It is configured only by flags and the scratch
# project: --additional-mcp-config @<scratch file>, --allow-tool delphi (the Delphi server's tools only: nothing else is
# approved, and a prompt-mode run denies what no rule allows), --trust-folder (this one session only; nothing saved),
# COPILOT_PLUGIN_DIR_ONLY=true (the operator's installed plugins are not loaded), --disable-builtin-mcps, --no-ask-user.
# Three model calls, the Auto router's efficiency profile unless COPILOT_PROBE_MODEL names a model.

. "$(dirname "$0")/lib.sh"
command -v copilot >/dev/null || die "copilot is not installed"
setup_scratch copilot
MODEL=${COPILOT_PROBE_MODEL:-auto}
export COPILOT_AUTO_TIER=${COPILOT_AUTO_TIER:-efficiency}
export COPILOT_PLUGIN_DIR_ONLY=true
export COPILOT_AUTO_UPDATE=false

cat >"$S/copilot-mcp.json" <<EOF
{ "mcpServers": { "delphi": { "type": "local", "command": "$S/bin/delphi", "args": ["mcp"], "tools": ["*"] } } }
EOF
write_instruction_canary AGENTS.md
write_instruction_canary .github/copilot-instructions.md
EXTRA=""
WORKER=delphi-architect-worker
if [ "$MODE" = final ]; then
    hooks_on_in_temp_home
    [ -n "${DELPHI_COPILOT_PLUGIN:-}" ] || die "final mode: set DELPHI_COPILOT_PLUGIN to the plugin folder delphi connect copilot wrote"
    EXTRA="--plugin-dir $DELPHI_COPILOT_PLUGIN"
else
    "$DELPHI_BIN" plugin export --host copilot --out "$S/export" >/dev/null
    mkdir -p "$P/.github/agents" "$P/.github/hooks"
    cp "$S/export/agents/delphi-architect-worker.agent.md" "$P/.github/agents/"
    # Both hook shapes: Copilot's documented one (camelCase events, `bash`) and Claude Code's (PascalCase, `command`).
    cat >"$P/.github/hooks/probe.json" <<EOF
{ "version": 1, "hooks": {
  "sessionStart": [ { "type": "command", "bash": "$S/bin/probe-hook sessionStart", "timeoutSec": 10 } ],
  "userPromptSubmitted": [ { "type": "command", "bash": "$S/bin/probe-hook userPromptSubmitted", "timeoutSec": 10 } ],
  "sessionEnd": [ { "type": "command", "bash": "$S/bin/probe-hook sessionEnd", "timeoutSec": 10 } ]
} }
EOF
    cat >"$P/.github/hooks/probe-claude-shape.json" <<EOF
{ "hooks": {
  "SessionStart": [ { "hooks": [ { "type": "command", "command": "$S/bin/probe-hook SessionStart" } ] } ],
  "UserPromptSubmit": [ { "hooks": [ { "type": "command", "command": "$S/bin/probe-hook UserPromptSubmit" } ] } ],
  "SessionEnd": [ { "hooks": [ { "type": "command", "command": "$S/bin/probe-hook SessionEnd" } ] } ]
} }
EOF
fi

run() {
    n=$1
    shift
    model_call
    # shellcheck disable=SC2086
    (cd "$P" && copilot -p "$1" --model "$MODEL" --additional-mcp-config "@$S/copilot-mcp.json" --allow-tool delphi \
        --trust-folder --disable-builtin-mcps --no-ask-user --no-auto-update --log-dir "$LOG/copilot-logs" --output-format json $EXTRA \
        >"$LOG/reply-$n.json" 2>"$LOG/reply-$n.err" </dev/null) || true
    collect "$n"
    printf '    COPILOT_PLUGIN_DIR_ONLY=true COPILOT_AUTO_TIER=%s copilot -p <prompt %s> --model %s --additional-mcp-config @<scratch>/copilot-mcp.json --allow-tool delphi --trust-folder --disable-builtin-mcps --no-ask-user --no-auto-update --log-dir <scratch>/log --output-format json %s\n' \
        "$COPILOT_AUTO_TIER" "$n" "$MODEL" "$(echo "$EXTRA" | sed "s#$S#<scratch>#")" >>"$LOG/commands.txt"
}

run 1 "$PROMPT_TOOLS"
run 2 "$PROMPT_MENTION"
run 3 "$(prompt_worker "$WORKER")"

summarize "Copilot CLI" "$(copilot --version 2>/dev/null | head -1)" "$WORKER"
