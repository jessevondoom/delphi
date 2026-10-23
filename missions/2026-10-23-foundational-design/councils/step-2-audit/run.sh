#!/bin/bash
# usage: run.sh <name> <model> ; reads prompts/common.txt + prompts/<name>.txt, writes <name>.raw.md
OUT="${AUDIT_OUT:?set AUDIT_OUT to a directory outside the repository}"
cd "$(dirname "$0")/../../../.." || exit 1
D=missions/2026-10-23-foundational-design/councils/step-2-audit
P="$(cat $D/prompts/common.txt $D/prompts/$1.txt)"
agy --print "$P" --model "$2" --mode plan --print-timeout 40m > "$OUT/$1-$2.raw.md" 2> "$OUT/$1-$2.err" &
PID=$!
( sleep 2700; kill $PID 2>/dev/null ) &
W=$!
wait $PID; RC=$?; kill $W 2>/dev/null
echo "$1 $2 exit $RC $(wc -w < "$OUT/$1-$2.raw.md") words" >> "$OUT/runs.log"
