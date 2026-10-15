#!/usr/bin/env sh
# Local packaging tests for install/install.sh. Does not hit GitHub Releases.
set -eu
ROOT=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
INSTALL="$ROOT/install/install.sh"
WORKDIR=$(mktemp -d)
trap 'rm -rf "$WORKDIR"' EXIT

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

make_dist() {
  dist=$1
  mkdir -p "$dist"
  printf '#!/bin/sh\necho agentos 0.1.0-test\n' > "$dist/agentos"
  chmod 755 "$dist/agentos"
  tar -C "$dist" -czf "$dist/agentos-aarch64-apple-darwin.tar.gz" agentos
  tar -C "$dist" -czf "$dist/agentos-x86_64-unknown-linux-gnu.tar.gz" agentos
  (
    cd "$dist"
    if command -v sha256sum >/dev/null 2>&1; then
      sha256sum agentos-*.tar.gz > SHA256SUMS
    else
      shasum -a 256 agentos-*.tar.gz > SHA256SUMS
    fi
  )
}

os=$(uname -s)
arch=$(uname -m)
case "$os:$arch" in
  Darwin:arm64) target=aarch64-apple-darwin ;;
  Darwin:x86_64) target=x86_64-apple-darwin ;;
  Linux:x86_64) target=x86_64-unknown-linux-gnu ;;
  Linux:aarch64|Linux:arm64) target=aarch64-unknown-linux-gnu ;;
  *)
    echo "SKIP: installer tests are not defined for $os $arch"
    exit 0
    ;;
esac

DIST="$WORKDIR/dist"
make_dist "$DIST"
# The helper always writes darwin-arm and linux-x86_64 archives. If this host
# needs another target, copy the linux archive under the detected name.
if [ ! -f "$DIST/agentos-${target}.tar.gz" ]; then
  src=$(ls "$DIST"/agentos-*.tar.gz | head -n 1)
  cp "$src" "$DIST/agentos-${target}.tar.gz"
  (
    cd "$DIST"
    if command -v sha256sum >/dev/null 2>&1; then
      sha256sum agentos-*.tar.gz > SHA256SUMS
    else
      shasum -a 256 agentos-*.tar.gz > SHA256SUMS
    fi
  )
fi

PREFIX="$WORKDIR/prefix"
AGENTOS_RELEASE_BASE="file://${DIST}" AGENTOS_PREFIX="$PREFIX" AGENTOS_TARGET="$target" \
  sh "$INSTALL"
[ -x "$PREFIX/bin/agentos" ] || fail "binary not installed"
out=$("$PREFIX/bin/agentos" --version)
[ "$out" = "agentos 0.1.0-test" ] || fail "version was $out"

AGENTOS_PREFIX="$PREFIX" sh "$INSTALL" --uninstall
[ ! -e "$PREFIX/bin/agentos" ] || fail "uninstall left the binary"

# Missing artifact
bad="$WORKDIR/empty"
mkdir -p "$bad"
if AGENTOS_RELEASE_BASE="file://${bad}" AGENTOS_PREFIX="$PREFIX" AGENTOS_TARGET="$target" \
  sh "$INSTALL" 2>"$WORKDIR/missing.err"; then
  fail "missing artifact should fail"
fi
grep -q "missing artifact" "$WORKDIR/missing.err" || fail "missing artifact error was unclear"

# Checksum mismatch
tamper="$WORKDIR/tamper"
cp -R "$DIST" "$tamper"
printf 'x' >> "$tamper/agentos-${target}.tar.gz"
if AGENTOS_RELEASE_BASE="file://${tamper}" AGENTOS_PREFIX="$PREFIX" AGENTOS_TARGET="$target" \
  sh "$INSTALL" 2>"$WORKDIR/mismatch.err"; then
  fail "checksum mismatch should fail"
fi
grep -q "checksum mismatch" "$WORKDIR/mismatch.err" || fail "checksum error was unclear"

# Latest without a release base fails clearly
if AGENTOS_PREFIX="$PREFIX" AGENTOS_VERSION=latest AGENTOS_TARGET="$target" \
  sh "$INSTALL" 2>"$WORKDIR/norelease.err"; then
  fail "unversioned install should fail without a release"
fi
grep -q "tagged" "$WORKDIR/norelease.err" || grep -q "release" "$WORKDIR/norelease.err" || fail "no-release error was unclear"

echo "install script tests passed"
