#!/usr/bin/env bash
# Renders every screen in mockups.html to a PNG next to this script, with headless Chrome at 3x
# (needs network once for the JetBrains Mono web font).
#   render.sh              # all
#   render.sh kb-2-results # one, by id
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
chrome="${CHROME:-/Applications/Google Chrome.app/Contents/MacOS/Google Chrome}"
[ -x "$chrome" ] || { echo "set CHROME to a Chrome/Chromium binary" >&2; exit 1; }
ids=(kb-1-resting kb-2-results kb-3-summary kb-4-reader kb-5-pdf kb-6-empty dec-1-queue dec-2-filtered dec-3-confirm dec-4-detail dec-5-norec dec-6-empty)
w=$(awk 'BEGIN{printf "%d", 100*7.8+32+4}')
h=$(awk 'BEGIN{printf "%d", 34*17.16+28+4}')
for id in "${ids[@]}"; do
  [ $# -gt 0 ] && [ "$1" != "$id" ] && continue
  "$chrome" --headless=new --disable-gpu --hide-scrollbars --force-device-scale-factor=3 \
    --virtual-time-budget=8000 --window-size="$w,$h" \
    --screenshot="$here/$id.png" "file://$here/mockups.html?shot=$id" >/dev/null 2>&1 || true
  [ -s "$here/$id.png" ] || { echo "$id: no PNG written" >&2; exit 1; }
  echo "$id.png"
done
