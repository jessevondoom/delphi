#!/usr/bin/env bash
# Decides whether a pull request changes only plain documentation, so that CI may skip the macOS jobs for it (spec 094
# §14). It prints `docs_only=true` or `docs_only=false` and appends the same line to $GITHUB_OUTPUT when that is set.
#
# Fails closed: the answer is `true` only when the event is a pull request, the list of changed files could be read, it
# is not empty, every entry is an added or modified file (no rename, copy, deletion or type change), and every path is
# on the allow list below. Anything else, including an error, is `false`, and the macOS jobs run.
#
# The allow list: Markdown that no program reads or embeds and no test reads in a way that could differ between Linux and
# macOS. The Linux jobs still run every test on these changes (the spec reference check, the compiled-spec check and the
# documentation tests among them), so what is skipped is only the second, macOS, run of the same checks.
#   README.md, CHANGELOG.md             the two root documents no build script or `include_str!` embeds
#   changelog.d/*.md                     release-note fragments
#   docs/**/*.md, except docs/reference/ guides and explanations (docs/reference/ is embedded by crates/delphi/build.rs
#                                        and generated from the program, so it is not on the list)
#   missions/**/*.md                     mission notes, plans and trackers
# Not on the list, on purpose: specs/, compiled/, skills/, agents/, themes/, schemas/ (read by the program, its lint and
# its tests), GOVERNANCE.md, SPEC.md, REVIEW.md (embedded in the binary), AGENTS.md and CONTRIBUTING.md (read by tests),
# every file that is not Markdown, and everything under crates/, ui/, app/, dev/ and .github/.
#
# Usage: docs_only.sh <event name> [<base> <head>]   (base and head default to HEAD^1 and HEAD: the pull request's test
# merge commit and its first parent, the base).
set -uo pipefail

event="${1:-}"
base="${2:-HEAD^1}"
head="${3:-HEAD}"

answer() {
  echo "docs_only=$1"
  echo "reason: $2"
  if [ -n "${GITHUB_OUTPUT:-}" ]; then
    echo "docs_only=$1" >> "$GITHUB_OUTPUT"
  fi
  exit 0
}

allowed() {
  # In a `case` pattern `*` also matches `/`, so `docs/*.md` covers every depth; the excluded directory comes first.
  case "$1" in
    docs/reference/*) return 1 ;;
    README.md | CHANGELOG.md) return 0 ;;
    changelog.d/*.md | docs/*.md | missions/*.md) return 0 ;;
    *) return 1 ;;
  esac
}

[ "$event" = "pull_request" ] || answer false "not a pull request ($event): the full set runs"

# --no-renames would turn a rename into a deletion and an addition; both are refused below either way. A path git has to
# quote (unusual characters) starts with a double quote, which no allowed pattern matches.
if ! changed="$(git diff --name-status "$base" "$head")"; then
  answer false "could not list the changed files"
fi
[ -n "$changed" ] || answer false "no changed files listed"

n=0
while IFS=$'\t' read -r status path rest; do
  n=$((n + 1))
  case "$status" in
    A | M) ;;
    *) answer false "$status $path${rest:+ -> $rest}: not an added or modified file" ;;
  esac
  allowed "$path" || answer false "$path is not on the documentation allow list"
done <<<"$changed"

answer true "all $n changed files are documentation on the allow list"
