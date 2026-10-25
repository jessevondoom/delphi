#!/usr/bin/env bash
# Renders every 005e mockup to a PNG next to this script, with headless Chrome (needs network once for the
# JetBrains Mono web font; otherwise Menlo is used and the glyph widths differ by a pixel or two).
#   specs/assets/005e/render.sh            # all
#   specs/assets/005e/render.sh epic-dark  # one, by file stem
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
chrome="${CHROME:-/Applications/Google Chrome.app/Contents/MacOS/Google Chrome}"
[ -x "$chrome" ] || { echo "set CHROME to a Chrome/Chromium binary" >&2; exit 1; }

# stem|view|theme|cols x rows|extra query
shots=(
  "chat-dark-100x34|chat|dark|100x34|scene=idle"
  "chat-light-100x34|chat|light|100x34|scene=idle"
  "chat-dark-80x24|chat|dark|80x24|scene=idle"
  "chat-streaming-dark-100x34|chat|dark|100x34|scene=stream"
  "chat-streaming-light-100x34|chat|light|100x34|scene=stream"
  "activity-dark-100x34|activity|dark|100x34|"
  "activity-light-100x34|activity|light|100x34|"
  "activity-dark-80x24|activity|dark|80x24|"
  "epic-dark|epic|dark|80x24|"
  "epic-light|epic|light|80x24|"
  "council-blind-dark|council|dark|80x24|cs=blind"
  "council-debate-dark|council|dark|80x24|cs=debate"
  "council-verdict-dark|council|dark|80x24|cs=verdict"
  "council-debate-light|council|light|80x24|cs=debate"
)

for s in "${shots[@]}"; do
  IFS='|' read -r stem view theme size extra <<<"$s"
  [ $# -gt 0 ] && [ "$1" != "$stem" ] && continue
  cols="${size%x*}"; rows="${size#*x}"
  w=$(awk -v c="$cols" 'BEGIN{printf "%d", c*7.8+32+4}')
  h=$(awk -v r="$rows" 'BEGIN{printf "%d", r*17.16+28+4}')
  "$chrome" --headless=new --disable-gpu --hide-scrollbars --force-device-scale-factor=2 \
    --virtual-time-budget=6000 --window-size="$w,$h" \
    --screenshot="$here/$stem.png" \
    "file://$here/mockups.html?shot=$view&theme=$theme&size=$size&$extra" >/dev/null 2>&1
  echo "$stem.png ${w}x${h}"
done
