#!/bin/bash
# One command: installs (once), starts a fake daemon, runs the UI from source. Quit with Ctrl+C.
set -e
cd "$(dirname "$0")"
[ -d node_modules/ink ] || npm ci --ignore-scripts --no-audit --no-fund
D=$(mktemp -d)
node dev/fake-info.mjs "$D/f.sock" > "$D/info.json"
node dev/fake-daemon.mjs "$D/f.sock" &
DP=$!
trap 'kill $DP 2>/dev/null; rm -rf "$D"' EXIT
sleep 0.5
node --import tsx src/main.tsx --info-file "$D/info.json"
