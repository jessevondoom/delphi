#!/bin/sh
# Build the desktop app here, with every local check (spec 111 §6): the window's gate, type check, tests and build; the
# browser check of the diagram renderer; the bridge's tests and clippy; the unsigned app; the smoke test of the built app.
# Needs Node 22.12 or newer, Rust (`cargo` on PATH), python3 and Chrome or Chromium (`CHROME=<path>`). Run from anywhere: `app/dev/build.sh`. Add `--no-smoke` on a machine with no display.
set -eu
cd "$(dirname "$0")/.."

npm ci --ignore-scripts --no-audit --no-fund
npm run check
# Hostile Mermaid diagrams drawn by the bundled renderer in headless Chrome (spec 118 §5 item 4); needs Chrome or Chromium.
npm run check-diagrams
(cd src-tauri && cargo test --locked && cargo clippy --locked --all-targets -- -D warnings)
npm run app
if [ "${1:-}" != "--no-smoke" ]; then
  python3 -I dev/smoke.py
fi
echo "built: $(cd .. && pwd)/target/app/release/bundle/macos/Delphi.app"
