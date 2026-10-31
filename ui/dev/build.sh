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
# OpenTUI imports one native package per platform; npm installs only the host's. Bundle the target's, leave the rest external.
if [ -n "$target" ]; then want="${target#bun-}"; else want="$(node -p "process.platform + '-' + (process.arch === 'x64' ? 'x64' : process.arch)")"; fi
if [ ! -d "node_modules/@opentui/core-$want" ]; then
  ver="$(node -p "require('./node_modules/@opentui/core/package.json').version")"
  npm install --no-save --force --ignore-scripts --no-audit --no-fund "@opentui/core-$want@$ver"
fi
for p in darwin-arm64 darwin-x64 linux-arm64 linux-arm64-musl linux-x64 linux-x64-musl win32-arm64 win32-x64; do
  [ "$p" = "$want" ] || args+=(--external "@opentui/core-$p")
done
node_modules/bun/bin/bun.exe "${args[@]}"
