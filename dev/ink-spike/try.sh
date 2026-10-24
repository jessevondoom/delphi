#!/bin/bash
# One command: installs (once), starts the fake daemon, runs the Ink client from source. Quit with Ctrl+C.
set -e
cd "$(dirname "$0")"
[ -d node_modules/ink ] || npm install --ignore-scripts --no-audit --no-fund
D=$(mktemp -d)
printf '#!/bin/sh\necho "{\\"console_socket\\":\\"%s/f.sock\\",\\"narrator_socket\\":\\"x\\"}"\n' "$D" > "$D/delphi"
chmod +x "$D/delphi"
node src/fake-daemon.mjs "$D/f.sock" &
DP=$!
trap 'kill $DP 2>/dev/null; rm -rf "$D"; stty sane 2>/dev/null; printf "\033[?1000l\033[?1006l\033[?1049l"' EXIT
sleep 0.5
DELPHI_BIN="$D/delphi" node --import tsx src/main.tsx
