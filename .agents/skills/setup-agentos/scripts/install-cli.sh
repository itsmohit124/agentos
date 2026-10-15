#!/usr/bin/env sh
# Install a published AgentOS CLI binary. This does not install skills.
# Shipped inside the setup-agentos skill so `npx skills add` copies it.
# Usage: install-cli.sh [--prefix DIR] [--version VERSION] [--uninstall]
# Local tests: AGENTOS_RELEASE_BASE=file:///path/to/dist
set -eu

REPO="${AGENTOS_REPO:-itsmohit124/agentos}"
PREFIX="${AGENTOS_PREFIX:-${HOME}/.local}"
VERSION="${AGENTOS_VERSION:-latest}"
UNINSTALL=0

while [ $# -gt 0 ]; do
  case "$1" in
    --prefix)
      PREFIX="${2:?--prefix needs a directory}"
      shift 2
      ;;
    --version)
      VERSION="${2:?--version needs a value}"
      shift 2
      ;;
    --uninstall)
      UNINSTALL=1
      shift
      ;;
    -h|--help)
      cat <<'EOF'
Install the AgentOS CLI from a GitHub Release.

  install.sh [--prefix DIR] [--version VERSION]
  install.sh --uninstall [--prefix DIR]

Environment:
  AGENTOS_REPO           GitHub owner/repo (default itsmohit124/agentos)
  AGENTOS_PREFIX         Install prefix (default ~/.local); binary goes to PREFIX/bin
  AGENTOS_VERSION        Tag such as v0.1.0, or latest
  AGENTOS_RELEASE_BASE   Override download root (tests use file:// or http://)
  AGENTOS_TARGET         Override rustc target triple

This script downloads a checksummed archive. It does not compile Rust.
npx skills add does not run this script.
EOF
      exit 0
      ;;
    *)
      echo "error: unknown argument $1" >&2
      exit 2
      ;;
  esac
done

BIN_DIR="${PREFIX}/bin"
BIN="${BIN_DIR}/agentos"

if [ "$UNINSTALL" -eq 1 ]; then
  if [ -e "$BIN" ]; then
    rm -f "$BIN"
    echo "removed $BIN"
  else
    echo "nothing to remove at $BIN"
  fi
  exit 0
fi

detect_target() {
  if [ -n "${AGENTOS_TARGET:-}" ]; then
    printf '%s\n' "$AGENTOS_TARGET"
    return
  fi
  os=$(uname -s)
  arch=$(uname -m)
  case "$os:$arch" in
    Darwin:arm64) echo aarch64-apple-darwin ;;
    Darwin:x86_64) echo x86_64-apple-darwin ;;
    Linux:x86_64) echo x86_64-unknown-linux-gnu ;;
    Linux:aarch64) echo aarch64-unknown-linux-gnu ;;
    Linux:arm64) echo aarch64-unknown-linux-gnu ;;
    MINGW*|MSYS*|CYGWIN*)
      echo "error: use install/install.ps1 on Windows" >&2
      exit 1
      ;;
    *)
      echo "error: unsupported platform $os $arch" >&2
      echo "Build from source: cargo build --release" >&2
      exit 1
      ;;
  esac
}

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  else
    echo "error: need sha256sum or shasum" >&2
    exit 1
  fi
}

fetch() {
  src=$1
  dest=$2
  case "$src" in
    file://*)
      path=${src#file://}
      cp "$path" "$dest"
      ;;
    *)
      if command -v curl >/dev/null 2>&1; then
        curl -fsSL "$src" -o "$dest"
      else
        echo "error: curl is required to download a release" >&2
        exit 1
      fi
      ;;
  esac
}

TARGET=$(detect_target)
ARCHIVE="agentos-${TARGET}.tar.gz"
SUMS="SHA256SUMS"

if [ -n "${AGENTOS_RELEASE_BASE:-}" ]; then
  BASE="${AGENTOS_RELEASE_BASE%/}"
else
  if [ "$VERSION" = "latest" ]; then
    echo "error: no GitHub Release URL override is set, and this installer needs a tagged asset." >&2
    echo "Set AGENTOS_VERSION to a published tag such as v0.1.0, or AGENTOS_RELEASE_BASE for a local test." >&2
    echo "Until a release exists, build with: cargo build --release" >&2
    exit 1
  fi
  BASE="https://github.com/${REPO}/releases/download/${VERSION}"
fi

WORKDIR=$(mktemp -d)
trap 'rm -rf "$WORKDIR"' EXIT

echo "downloading ${ARCHIVE} from ${BASE}"
if ! fetch "${BASE}/${ARCHIVE}" "${WORKDIR}/${ARCHIVE}"; then
  echo "error: missing artifact ${ARCHIVE} at ${BASE}" >&2
  exit 1
fi
if ! fetch "${BASE}/${SUMS}" "${WORKDIR}/${SUMS}"; then
  echo "error: missing checksum file ${SUMS} at ${BASE}" >&2
  exit 1
fi

expected=$(awk -v name="$ARCHIVE" '$2 == name {print $1}' "${WORKDIR}/${SUMS}")
if [ -z "$expected" ]; then
  echo "error: ${ARCHIVE} is not listed in ${SUMS}" >&2
  exit 1
fi
actual=$(sha256_file "${WORKDIR}/${ARCHIVE}")
if [ "$actual" != "$expected" ]; then
  echo "error: checksum mismatch for ${ARCHIVE}" >&2
  echo "  expected ${expected}" >&2
  echo "  actual   ${actual}" >&2
  exit 1
fi

tar -xzf "${WORKDIR}/${ARCHIVE}" -C "$WORKDIR"
if [ ! -x "${WORKDIR}/agentos" ]; then
  echo "error: archive did not contain an executable named agentos" >&2
  exit 1
fi

mkdir -p "$BIN_DIR"
# Do not overwrite a path that is not an agentos binary if it is a directory.
if [ -d "$BIN" ]; then
  echo "error: ${BIN} is a directory" >&2
  exit 1
fi
cp "${WORKDIR}/agentos" "$BIN"
chmod 755 "$BIN"

echo "installed ${BIN}"
"$BIN" --version
echo "Add ${BIN_DIR} to PATH if agentos is not found."
