#!/bin/bash
# usage: run.sh <seat> <model>   (agy seats only; Claude seats run as Claude Code subagents)
cd "$(dirname "$0")/../../../.." || exit 1
D=missions/2026-10-23-foundational-design/councils/010
P="$(cat $D/prompts/common.txt $D/prompts/$1.txt)"
agy --print "$P" --model "$2" --mode plan --print-timeout 30m > "$D/round1-$1-$2.md" 2>> "$D/errors.log"
echo "$1 $2 exit=$?" >> "$D/prompts/status"
