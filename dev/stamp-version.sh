#!/bin/sh
# Stamp a development version into crates/delphi/Cargo.toml and Cargo.lock (so `--locked` still holds) and print it.
#   dev/stamp-version.sh <commit>      ->  0.2.4-dev.g0123456
# Used by .github/workflows/dev.yml. Plain awk and mv only: it must behave the same on the macOS and Linux runners.
set -eu
cd "$(dirname "$0")/.."
[ $# -eq 1 ] || { echo "usage: dev/stamp-version.sh <commit>" >&2; exit 2; }
short="$(printf '%s' "$1" | cut -c1-7)"
printf '%s' "$short" | grep -Eq '^[0-9a-f]{7}$' || { echo "not a commit: $1" >&2; exit 2; }
base="$(awk '/^\[package\]/{f=1} f && /^version/{gsub(/"/,"",$3); print $3; exit}' crates/delphi/Cargo.toml)"
printf '%s' "$base" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$' || { echo "crate version is not MAJOR.MINOR.PATCH: $base" >&2; exit 1; }
# The `g` (as in `git describe`) keeps an all-digit commit with a leading zero a valid semver identifier.
ver="$base-dev.g$short"
awk -v v="$ver" '/^\[package\]/{f=1} f && /^version/ && !done{print "version = \"" v "\""; done=1; next} {print}' crates/delphi/Cargo.toml > crates/delphi/Cargo.toml.new
mv crates/delphi/Cargo.toml.new crates/delphi/Cargo.toml
awk -v b="$base" -v v="$ver" '/^name = "delphi"$/{hit=1} hit && $0 == "version = \"" b "\""{print "version = \"" v "\""; hit=0; next} {print}' Cargo.lock > Cargo.lock.new
mv Cargo.lock.new Cargo.lock
grep -q "^version = \"$ver\"" crates/delphi/Cargo.toml && grep -q "^version = \"$ver\"" Cargo.lock || { echo "could not stamp $ver" >&2; exit 1; }
echo "$ver"
