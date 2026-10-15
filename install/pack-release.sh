#!/usr/bin/env sh
# Package target/release/agentos into dist/ for a GitHub Release.
set -eu
ROOT=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
TARGET="${1:?usage: pack-release.sh <rustc-target>}"
VERSION="${2:-${AGENTOS_VERSION:-0.1.0}}"
OUT="${3:-${ROOT}/dist}"
BIN="${CARGO_TARGET_DIR:-$ROOT/target}/release/agentos"
WIN_BIN="${CARGO_TARGET_DIR:-$ROOT/target}/release/agentos.exe"
if [ -n "${CARGO_TARGET_DIR:-}" ]; then
  if [ -x "${CARGO_TARGET_DIR}/${TARGET}/release/agentos" ]; then
    BIN="${CARGO_TARGET_DIR}/${TARGET}/release/agentos"
    WIN_BIN="${CARGO_TARGET_DIR}/${TARGET}/release/agentos.exe"
  fi
elif [ -x "$ROOT/target/${TARGET}/release/agentos" ]; then
  BIN="$ROOT/target/${TARGET}/release/agentos"
  WIN_BIN="$ROOT/target/${TARGET}/release/agentos.exe"
fi

mkdir -p "$OUT"
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
case "$TARGET" in
  *windows*)
    [ -x "$WIN_BIN" ] || [ -f "$WIN_BIN" ] || {
      echo "error: missing $WIN_BIN" >&2
      exit 1
    }
    cp "$WIN_BIN" "$stage/agentos.exe"
    python3 - "$stage/agentos.exe" "$OUT/agentos-${TARGET}.zip" <<'PY'
import sys, zipfile
src, dest = sys.argv[1], sys.argv[2]
with zipfile.ZipFile(dest, "w", zipfile.ZIP_DEFLATED) as archive:
    archive.write(src, "agentos.exe")
PY
    ;;
  *)
    [ -x "$BIN" ] || {
      echo "error: missing $BIN" >&2
      exit 1
    }
    cp "$BIN" "$stage/agentos"
    chmod 755 "$stage/agentos"
    tar -C "$stage" -czf "$OUT/agentos-${TARGET}.tar.gz" agentos
    ;;
esac

(
  cd "$OUT"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum agentos-* > SHA256SUMS
  else
    shasum -a 256 agentos-* > SHA256SUMS
  fi
)
echo "packed $TARGET version $VERSION into $OUT"
