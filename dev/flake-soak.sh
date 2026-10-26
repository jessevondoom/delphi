#!/bin/sh
# Run a set of tests over and over, under CPU load, and report how many runs failed.
#
#   dev/flake-soak.sh [runs] [load] -- <cargo test args after --test-threads>
#
# Defaults: 20 runs, 12 busy loops, and the pty console filter CI uses. Use it before merging a change to the console
# or to a test harness: before the #154 fixes about 13% of runs failed on an 8-core Mac, after them 0 of 40.
# Exits non-zero if any run failed; failing output is kept under $SOAK_OUT (default: a temp dir) and its path printed.
set -u
runs="${1:-20}"
load="${2:-12}"
[ $# -ge 2 ] && shift 2 || shift $#
[ "${1:-}" = "--" ] && shift
filter="${*:-full_screen esc_stops hands_over decision_inbox full_screen_observer research_panels terminal_is_given_back survives_a_daemon_restart}"
out="${SOAK_OUT:-$(mktemp -d)}"
mkdir -p "$out"

cd "$(dirname "$0")/.." || exit 2
[ -d ui/node_modules/ink ] || echo "warning: ui/node_modules is missing, the pty tests will skip and prove nothing" >&2
# shellcheck disable=SC2086
bin="$(cargo test --locked -p delphi --test e2e --no-run --message-format=json 2>/dev/null |
  python3 -I -c 'import sys,json
for l in sys.stdin:
    try: m=json.loads(l)
    except ValueError: continue
    if m.get("executable") and m.get("target",{}).get("name")=="e2e": print(m["executable"])' | tail -1)"
[ -n "$bin" ] || { echo "could not build the e2e test binary" >&2; exit 2; }

pids=""
i=0
while [ "$i" -lt "$load" ]; do
  (while :; do :; done) &
  pids="$pids $!"
  i=$((i + 1))
done
trap 'kill $pids 2>/dev/null' EXIT INT TERM

failed=0
n=1
while [ "$n" -le "$runs" ]; do
  # shellcheck disable=SC2086
  if "$bin" --test-threads=10 $filter >"$out/run-$n.log" 2>&1; then
    rm -f "$out/run-$n.log"
  else
    failed=$((failed + 1))
    echo "run $n failed: $(grep -E '^test .* FAILED' "$out/run-$n.log" | tr '\n' ' ')"
  fi
  n=$((n + 1))
done
echo "$failed of $runs runs failed (logs of failures in $out)"
[ "$failed" -eq 0 ]
