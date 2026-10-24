#!/bin/bash
# usage: run.sh <round> <seat> <model>
# prompt = prompts/r<round>-common.txt (else common.txt) + prompts/r<round>-<seat>.txt (else <seat>.txt)
cd "$(dirname "$0")/../../../.." || exit 1
D=missions/2026-10-23-foundational-design/councils/008
C=common.txt; [ -f "$D/prompts/r$1-common.txt" ] && C="r$1-common.txt"
S="$2.txt"; [ -f "$D/prompts/r$1-$2.txt" ] && S="r$1-$2.txt"
P="$(cat $D/prompts/$C $D/prompts/$S)"
agy --print "$P" --model "$3" --mode plan --print-timeout 30m > "$D/round$1-$2.md" 2>> "$D/errors.log"
echo "$2 r$1 exit=$?" >> "$D/prompts/status"
