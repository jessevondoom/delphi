#!/bin/bash
# Try the narrator's panes with no daemon and no model: an animated mission is simulated (ui/dev/preview-sim.ts) and the real
# narrator runs against it. Left and right arrows change the pane, up and down scroll, Ctrl+C quits.
#
#   ui/dev/preview.sh [pane] [--calm] [--seed N]
#
# pane is one of: dashboard council epic agents pulse timeline tools archives changes (default: pulse).
# --calm leaves out the approval, the loop and the file clash, to see the quiet picture. Nothing is written outside a temp directory.
set -e
cd "$(dirname "$0")/.."
[ -d node_modules/@opentui/core ] || npm ci --ignore-scripts --no-audit --no-fund
PANE=pulse
case "${1:-}" in ''|--*) ;; *) PANE="$1"; shift ;; esac
D=$(mktemp -d)
node dev/fake-info.mjs "$D/p.sock" > "$D/info.json"
node --import tsx dev/preview-daemon.ts "$D/p.sock" "$@" &
DP=$!
trap 'kill $DP 2>/dev/null; rm -rf "$D"' EXIT
sleep 1
node --experimental-ffi --import ./dev/check-runtime.mjs --import tsx src/main.tsx --info-file "$D/info.json" --observe "$PANE"
