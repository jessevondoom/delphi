#!/bin/bash
# usage: run.sh <round> <seat> <model>; prompt = prompts/common(.r2).txt + prompts/<seat>.txt
cd "$(dirname "$0")/../../../.." || exit 1
D=missions/2026-10-23-foundational-design/councils/006
C=common.txt; [ "$1" = 2 ] && C=r2-common.txt
P="$(cat $D/prompts/$C $D/prompts/$2.txt)"
agy --print "$P" --model "$3" --mode plan --print-timeout 30m > "$D/round$1-$2.md" 2>> "$D/errors.log"
