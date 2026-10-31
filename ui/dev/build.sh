#!/bin/bash
# Build the full-screen console as one executable. usage: dev/build.sh <version> <out> [bun-target]
#   dev/build.sh 0.1.0 dist/delphi-ui                     (this machine)
#   dev/build.sh 0.1.0 dist/delphi-ui bun-linux-arm64     (cross-compile)
# The version is baked in; the console refuses to run beside a `delphi` of another release.
set -euo pipefail
cd "$(dirname "$0")/.."
version="$1"; out="$2"; target="${3:-}"
[ -d node_modules/@opentui/core ] || npm ci --ignore-scripts --no-audit --no-fund
# `npm ci --ignore-scripts` skips the bun package's own install step, which places its platform binary.
[ -x node_modules/bun/bin/bun.exe ] && node_modules/bun/bin/bun.exe --version >/dev/null 2>&1 || node node_modules/bun/install.js
mkdir -p "$(dirname "$out")"
args=(build --compile --define "BUILD_VERSION=\"$version\"" src/main.tsx --outfile "$out")
[ -z "$target" ] || args+=(--target="$target")
node_modules/bun/bin/bun.exe "${args[@]}"
