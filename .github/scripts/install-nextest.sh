#!/usr/bin/env bash
# Installs cargo-nextest from its release archive at a pinned version, refusing an archive whose SHA-256 differs from the one
# written in the workflow (spec 125 amendment 125-A1). The workflow step sets:
#   NEXTEST_VERSION  e.g. 0.9.150
#   NEXTEST_TARGET   x86_64-unknown-linux-gnu or universal-apple-darwin
#   NEXTEST_SHA256   the archive's SHA-256, taken once from the release and checked against its published .sha256 file
# To move to a new version, change all three in every job that installs it (ci.yml), with the new archives' hashes.
set -euo pipefail

: "${NEXTEST_VERSION:?}" "${NEXTEST_TARGET:?}" "${NEXTEST_SHA256:?}"
archive="cargo-nextest-${NEXTEST_VERSION}-${NEXTEST_TARGET}.tar.gz"
url="https://github.com/nextest-rs/nextest/releases/download/cargo-nextest-${NEXTEST_VERSION}/${archive}"
dest="${RUNNER_TEMP:-/tmp}/${archive}"

# A download can fail for a moment; 3 attempts, 20 seconds apart.
for attempt in 1 2 3; do
  if curl --proto '=https' --tlsv1.2 -fsSL -o "$dest" "$url"; then
    break
  fi
  if [ "$attempt" -eq 3 ]; then
    echo "could not download $url after 3 attempts"
    exit 1
  fi
  echo "download failed (attempt $attempt of 3); retrying in 20s"
  sleep 20
done

echo "${NEXTEST_SHA256}  ${dest}" | shasum -a 256 -c -
mkdir -p "$HOME/.cargo/bin"
tar -xzf "$dest" -C "$HOME/.cargo/bin" cargo-nextest
cargo nextest --version
