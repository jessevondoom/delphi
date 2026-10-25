#!/bin/sh
# Make the preloaded vectors for the documents Delphi ships (specs/004 §3.6), and check them.
#
#   dev/build-seed-vectors.sh <model package> <out file> [<delphi executable>]
#
# A fresh workspace is created with the executable's own seed, indexed with the given model, and its vectors are
# exported. The result is what `--features embedded-model` compiles in (DELPHI_SEED_VECTORS_FILE=<out file>).
# Then the check: a second fresh workspace is indexed with the cache and the model must read nothing at all.
# Run it after any change to the shipped agents, skills, context or memory templates, with the model that ships.
set -eu

model="${1:?usage: dev/build-seed-vectors.sh <model package> <out file> [<delphi executable>]}"
out="${2:?usage: dev/build-seed-vectors.sh <model package> <out file> [<delphi executable>]}"
root="$(cd "$(dirname "$0")/.." && pwd)"
delphi="${3:-}"
if [ -z "$delphi" ]; then
  (cd "$root" && cargo build -q --release --locked -p delphi)
  delphi="$root/target/release/delphi"
fi
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/project"

fresh_workspace() { # <home>
  mkdir -p "$1"
  (cd "$work/project" && printf 'seed\n\n' | "$delphi" init --home "$1" >/dev/null)
}

# 1. Embed everything with the model, and export.
fresh_workspace "$work/home-a"
DELPHI_EMBED_MODEL="$model" DELPHI_SEED_VECTORS=off "$delphi" kb index --home "$work/home-a" --workspace seed
DELPHI_EMBED_MODEL="$model" DELPHI_SEED_VECTORS=off "$delphi" kb seed-export "$out" --home "$work/home-a" --workspace seed

# 2. A new workspace with the cache must not need the model for the shipped documents.
fresh_workspace "$work/home-b"
report="$(DELPHI_EMBED_MODEL="$model" DELPHI_SEED_VECTORS="$out" "$delphi" kb index --home "$work/home-b" --workspace seed 2>&1 | tr '\r' '\n')"
echo "$report" | tail -3
echo "$report" | grep -q ', 0 by the model' || { echo "build-seed-vectors: the model still had to read some shipped text; the cache does not cover the seed" >&2; exit 1; }
