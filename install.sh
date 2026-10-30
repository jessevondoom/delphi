#!/bin/sh
# Install Delphi: download the release binary for this machine, check it, put it on your PATH, offer to run setup.
#
#   curl -fsSL https://github.com/jessevondoom/delphi/releases/latest/download/install.sh | sh
#
# Options (as arguments after `sh -s --`, or as environment variables):
#   --version vX.Y.Z   (DELPHI_VERSION)      install that release instead of the latest
#   --dir DIR          (DELPHI_INSTALL_DIR)  install into DIR (default: ~/.local/bin)
#   --no-setup                               do not offer to run `delphi setup` afterwards
# Maintainers / mirrors: DELPHI_API (releases API base) and DELPHI_DOWNLOAD (asset base URL) override the defaults.
#
# The download is checked against its published SHA-256 and its build-provenance attestation (needs the `gh` CLI,
# signed in). Without gh the install stops; DELPHI_SKIP_ATTESTATION=1 accepts the checksum alone.
set -eu

REPO="jessevondoom/delphi"
API="${DELPHI_API:-https://api.github.com/repos/$REPO}"
DOWNLOAD="${DELPHI_DOWNLOAD:-https://github.com/$REPO/releases/download}"
# Every request is HTTPS-only. A mirror override must be HTTPS too (plain http only with DELPHI_INSECURE_TEST=1,
# for local testing), and says so loudly.
PROTO="=https"
if [ -n "${DELPHI_API:-}${DELPHI_DOWNLOAD:-}" ]; then
  printf 'install.sh: WARNING: using a custom release source (DELPHI_API / DELPHI_DOWNLOAD)\n' >&2
  case "$API$DOWNLOAD" in
    http://*|*http://*) [ "${DELPHI_INSECURE_TEST:-0}" = "1" ] || { printf 'install.sh: custom sources must be https\n' >&2; exit 1; }; PROTO="=http,https" ;;
  esac
fi
fetch() { curl -fsSL --proto "$PROTO" --tlsv1.2 "$@"; }
VERSION="${DELPHI_VERSION:-}"
DIR="${DELPHI_INSTALL_DIR:-$HOME/.local/bin}"
SETUP=1

say() { printf '%s\n' "$*"; }
die() { printf 'install.sh: %s\n' "$*" >&2; exit 1; }

while [ $# -gt 0 ]; do
  case "$1" in
    --version) [ $# -ge 2 ] || die "--version needs a value"; VERSION="$2"; shift 2 ;;
    --dir) [ $# -ge 2 ] || die "--dir needs a value"; DIR="$2"; shift 2 ;;
    --no-setup) SETUP=0; shift ;;
    -h|--help) sed -n '2,13p' "$0" 2>/dev/null || true; exit 0 ;;
    *) die "unknown option: $1" ;;
  esac
done

command -v curl >/dev/null 2>&1 || die "curl is required"
command -v tar >/dev/null 2>&1 || die "tar is required"

os="$(uname -s)"; arch="$(uname -m)"
case "$os/$arch" in
  Darwin/arm64) target="aarch64-apple-darwin" ;;
  Darwin/x86_64) target="x86_64-apple-darwin" ;;
  Linux/x86_64) target="x86_64-unknown-linux-gnu" ;;
  Linux/aarch64|Linux/arm64) target="aarch64-unknown-linux-gnu" ;;
  *) die "no build for $os/$arch (macOS and Linux on x86_64 or arm64 are supported)" ;;
esac

if [ -z "$VERSION" ]; then
  say "Finding the latest release..."
  VERSION="$(fetch "$API/releases/latest" | sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -n 1)"
  [ -n "$VERSION" ] || die "could not find the latest release (is anything published yet?)"
fi
case "$VERSION" in *[!A-Za-z0-9.+-]*) die "DELPHI_VERSION has characters a version cannot have" ;; esac
case "$VERSION" in v*) ;; *) VERSION="v$VERSION" ;; esac
printf '%s' "$VERSION" | grep -Eq '^v[0-9]+\.[0-9]+\.[0-9]+(-[A-Za-z0-9.]+)?$' || die "odd version string: $VERSION (expected vMAJOR.MINOR.PATCH)"

name="delphi-$VERSION-$target.tar.gz"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT INT TERM

say "Downloading $name ..."
fetch -o "$tmp/$name" "$DOWNLOAD/$VERSION/$name" || die "download failed: $DOWNLOAD/$VERSION/$name"
fetch -o "$tmp/$name.sha256" "$DOWNLOAD/$VERSION/$name.sha256" || die "the checksum file is missing; refusing to install without it"

want="$(cut -d' ' -f1 "$tmp/$name.sha256" | tr 'A-F' 'a-f')"
if command -v sha256sum >/dev/null 2>&1; then got="$(sha256sum "$tmp/$name" | cut -d' ' -f1)"
elif command -v shasum >/dev/null 2>&1; then got="$(shasum -a 256 "$tmp/$name" | cut -d' ' -f1)"
else die "need sha256sum or shasum to verify the download"; fi
[ "${#want}" -eq 64 ] && [ "$want" = "$got" ] || die "checksum mismatch (wanted $want, got $got); nothing was installed"
say "Checksum OK."

# The checksum only proves the download is intact: it comes from the same place as the file. The real check is the
# build-provenance attestation (made by this repo's release workflow), which `gh` can verify. If gh is installed and
# signed in, it is verified and a failure stops the install. Without gh the install stops too, because a checksum
# alone cannot tell you who built the file; DELPHI_SKIP_ATTESTATION=1 is the explicit, noisy way past that.
if [ "${DELPHI_INSECURE_TEST:-0}" = "1" ]; then
  say "WARNING: DELPHI_INSECURE_TEST=1, so the attestation is not checked."
elif command -v gh >/dev/null 2>&1 && gh auth status >/dev/null 2>&1; then
  gh attestation verify "$tmp/$name" --repo "$REPO" --signer-workflow "$REPO/.github/workflows/release.yml" >/dev/null 2>&1 \
    || die "the build-provenance attestation did not verify; this download was not built by the release workflow. Nothing was installed."
  say "Attestation OK (built by the release workflow)."
elif [ "${DELPHI_SKIP_ATTESTATION:-0}" = "1" ]; then
  say "WARNING: DELPHI_SKIP_ATTESTATION=1, so only the checksum was verified. It cannot tell you who built this file."
else
  die "cannot verify who built this download: the gh CLI is missing or not signed in (install it, then gh auth login). To install on the checksum alone, run again with DELPHI_SKIP_ATTESTATION=1. Nothing was installed."
fi

# Extract exactly two paths (the program and its full-screen console), and only if they are regular files.
tar -xzf "$tmp/$name" -C "$tmp" "delphi-$VERSION-$target/delphi" || die "the archive has no delphi executable"
bin="$tmp/delphi-$VERSION-$target/delphi"
[ -f "$bin" ] && [ ! -L "$bin" ] || die "the archive's delphi is not a regular file"
"$bin" --version >/dev/null 2>&1 || die "the downloaded binary does not run on this machine"
# Older releases have no console; the program then falls back to its inline one.
ui=""
if tar -xzf "$tmp/$name" -C "$tmp" "delphi-$VERSION-$target/delphi-ui" 2>/dev/null; then
  ui="$tmp/delphi-$VERSION-$target/delphi-ui"
  [ -f "$ui" ] && [ ! -L "$ui" ] || die "the archive's delphi-ui is not a regular file"
fi

mkdir -p "$DIR"
[ ! -L "$DIR/delphi" ] || die "$DIR/delphi is a symlink; remove it first"
stage="$(mktemp "$DIR/.delphi.XXXXXX")"
trap 'rm -rf "$tmp"; rm -f "$stage"' EXIT INT TERM
cp "$bin" "$stage" || die "could not copy the binary into $DIR"
chmod 755 "$stage" || die "could not make the binary executable"
mv -f "$stage" "$DIR/delphi" || die "could not move the binary into place"
if [ -n "$ui" ]; then
  [ ! -L "$DIR/delphi-ui" ] || die "$DIR/delphi-ui is a symlink; remove it first"
  stage_ui="$(mktemp "$DIR/.delphi-ui.XXXXXX")"
  trap 'rm -rf "$tmp"; rm -f "$stage" "$stage_ui"' EXIT INT TERM
  cp "$ui" "$stage_ui" && chmod 755 "$stage_ui" && mv -f "$stage_ui" "$DIR/delphi-ui" || die "could not install delphi-ui into $DIR"
fi
say "Installed $("$DIR/delphi" --version) to $DIR/delphi"

case ":$PATH:" in
  *":$DIR:"*) ;;
  *) say ""; say "$DIR is not on your PATH. Add this line to your shell profile (~/.zshrc or ~/.bashrc):"; printf '  export PATH="%s:$PATH"\n' "$DIR" ;;
esac

if [ "$SETUP" -eq 1 ] && ( : < /dev/tty ) 2>/dev/null; then
  printf '\nRun `delphi` now to finish setting up? [Y/n] ' > /dev/tty
  read -r answer < /dev/tty || answer=""
  case "$answer" in n|N|no|No) say "Run \`delphi\` in any directory whenever you are ready." ;; *) "$DIR/delphi" < /dev/tty || say "Setup did not finish; run \`delphi\` again when you are ready." ;; esac
else
  say ""; say "Next: run \`delphi\` in any directory."
fi
