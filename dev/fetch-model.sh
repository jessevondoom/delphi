#!/bin/sh
# Fetch a model at a pinned revision, check it, and pack it for the binary (specs/004 §3.5 and §3.7).
#
#   dev/fetch-model.sh <embed|rerank> <out file> [<delphi executable>]
#
# `embed` is the embedding model (what `--features embedded-model` compiles in as DELPHI_EMBED_MODEL_FILE; also
# what DELPHI_EMBED_MODEL or $DELPHI_HOME/models/embed.dlphmdl can point at). `rerank` is the cross-encoder that
# reorders the best search results (DELPHI_RERANK_MODEL_FILE; $DELPHI_HOME/models/rerank.dlphmdl). Without an
# executable argument, delphi is built from this checkout to do the packing.
#
# To change a model: change its REPO, REV and SHA-256 sums together, bump the id in its pack line (the id is
# what makes an index remake its vectors), and run `delphi kb eval` before and after.
set -eu

which="${1:?usage: dev/fetch-model.sh <embed|rerank> <out file> [<delphi executable>]}"
out="${2:?usage: dev/fetch-model.sh <embed|rerank> <out file> [<delphi executable>]}"
delphi="${3:-}"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

case "$which" in
  embed)
    REPO="BAAI/bge-base-en-v1.5"
    REV="a5beb1e3e68b9ab74eb54cfd186867f64f240e1a"
    SUM_CONFIG="bc00af31a4a31b74040d73370aa83b62da34c90b75eb77bfa7db039d90abd591"
    SUM_TOKENIZER="d241a60d5e8f04cc1b2b3e9ef7a4921b27bf526d9f6050ab90f9267a1f9e5c66"
    SUM_WEIGHTS="c7c1988aae201f80cf91a5dbbd5866409503b89dcaba877ca6dba7dd0a5167d7"
    PACK="--id bge-base-en-v1.5-q8 --kind bert --pooling cls --dim 768 --max-tokens 512 --min-similarity 0.54"
    ;;
  rerank)
    REPO="cross-encoder/ms-marco-MiniLM-L12-v2"
    REV="7b0235231ca2674cb8ca8f022859a6eba2b1c968"
    SUM_CONFIG="69a745055b0307584d2903ea5cf4e1899254e511834f358be80d049918b144d8"
    SUM_TOKENIZER="d241a60d5e8f04cc1b2b3e9ef7a4921b27bf526d9f6050ab90f9267a1f9e5c66"
    SUM_WEIGHTS="1ed84b90cdf3518f76ec9bb93a16f97887eea7c4e7ee5dfb03cc297e394fbbc3"
    PACK="--id ms-marco-minilm-l12-v2-q8 --kind bert-cross --dim 1 --max-tokens 512 --min-similarity -9"
    ;;
  *)
    echo "usage: dev/fetch-model.sh <embed|rerank> <out file> [<delphi executable>]" >&2
    exit 2
    ;;
esac

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
  # shellcheck disable=SC2086
  "$@" kb model pack "$work" "$out" $PACK --source "$REPO@$REV" \
    ${QUERY_PREFIX:+--query-prefix "$QUERY_PREFIX"}
}

[ "$which" = embed ] && QUERY_PREFIX="Represent this sentence for searching relevant passages: "

if [ -n "$delphi" ]; then
  pack "$delphi"
else
  pack cargo run -q --release --locked -p delphi --
fi
