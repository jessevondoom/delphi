#!/bin/sh
# Fetch the embedding model at a pinned revision, check it, and pack it for the binary (specs/004 §3.5).
#
#   dev/fetch-model.sh <out file> [<delphi executable>]
#
# The package is what `--features embedded-model` compiles in (DELPHI_EMBED_MODEL_FILE=<out file>), and what
# DELPHI_EMBED_MODEL or $DELPHI_HOME/models/embed.dlphmdl can point at. Without an executable argument, delphi is
# built from this checkout to do the packing.
#
# To change the model: change REPO, REV and the SHA-256 sums together, bump the id in the pack line (the id is
# what makes an index remake its vectors), and run `delphi kb eval` before and after.
set -eu

REPO="BAAI/bge-base-en-v1.5"
REV="a5beb1e3e68b9ab74eb54cfd186867f64f240e1a"
SUM_CONFIG="bc00af31a4a31b74040d73370aa83b62da34c90b75eb77bfa7db039d90abd591"
SUM_TOKENIZER="d241a60d5e8f04cc1b2b3e9ef7a4921b27bf526d9f6050ab90f9267a1f9e5c66"
SUM_WEIGHTS="c7c1988aae201f80cf91a5dbbd5866409503b89dcaba877ca6dba7dd0a5167d7"

out="${1:?usage: dev/fetch-model.sh <out file> [<delphi executable>]}"
delphi="${2:-}"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

sha256() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | cut -d' ' -f1; else shasum -a 256 "$1" | cut -d' ' -f1; fi
}

fetch() { # <file name> <expected sha256>
  curl -fsSL --retry 3 -o "$work/$1" "https://huggingface.co/$REPO/resolve/$REV/$1"
  got="$(sha256 "$work/$1")"
  [ "$got" = "$2" ] || { echo "fetch-model: $1 has sha256 $got, expected $2" >&2; exit 1; }
}

fetch config.json "$SUM_CONFIG"
fetch tokenizer.json "$SUM_TOKENIZER"
fetch model.safetensors "$SUM_WEIGHTS"

pack() {
  "$@" kb model pack "$work" "$out" \
    --id bge-base-en-v1.5-q8 --kind bert --pooling cls --dim 768 --max-tokens 512 --min-similarity 0.54 \
    --query-prefix "Represent this sentence for searching relevant passages: " \
    --source "$REPO@$REV"
}

if [ -n "$delphi" ]; then
  pack "$delphi"
else
  pack cargo run -q --release --locked -p delphi --
fi
