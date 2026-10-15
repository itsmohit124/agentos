#!/usr/bin/env sh
# First-run: install skills (if missing), ensure the CLI, then init this project.
# Run this from the project you want to analyze.
# Usage: sh bootstrap.sh [--root DIR] [--prefix DIR] [--skip-skills] [--skip-init]
set -eu

HERE=$(CDPATH= cd -- "$(dirname "$0")" && pwd)
SRC=$(CDPATH= cd -- "$HERE/.." && pwd)
ROOT="${AGENTOS_ROOT:-.}"
PREFIX="${AGENTOS_PREFIX:-${HOME}/.local}"
SKIP_SKILLS=0
SKIP_INIT=0
REPO="${AGENTOS_REPO:-itsmohit124/agentos}"

while [ $# -gt 0 ]; do
  case "$1" in
    --root)
      ROOT="${2:?--root needs a directory}"
      shift 2
      ;;
    --prefix)
      PREFIX="${2:?--prefix needs a directory}"
      shift 2
      ;;
    --skip-skills)
      SKIP_SKILLS=1
      shift
      ;;
    --skip-init)
      SKIP_INIT=1
      shift
      ;;
    -h|--help)
      cat <<'EOF'
Set up AgentOS in the current project in one step: skills, CLI, init.

  sh install/bootstrap.sh [--root DIR] [--prefix DIR]
  sh install/bootstrap.sh --skip-skills
  sh install/bootstrap.sh --skip-init

This is the command that does first-run work. `npx skills add` only copies
SKILL.md files; it is not this script.

CLI resolution order:
  1. AGENTOS_BIN, or agentos already on PATH, or PREFIX/bin/agentos
  2. A release binary via install/install.sh (checksummed)
  3. cargo build --release in this source tree, AGENTOS_SRC, or a cache clone

Until a GitHub Release exists, step 2 fails and step 3 needs Rust.
EOF
      exit 0
      ;;
    *)
      echo "error: unknown argument $1" >&2
      exit 2
      ;;
  esac
done

ROOT=$(CDPATH= cd -- "$ROOT" && pwd)
BIN_DIR="${PREFIX}/bin"
mkdir -p "$BIN_DIR"

echo "project: $ROOT"

install_skills() {
  if [ "$SKIP_SKILLS" -eq 1 ]; then
    echo "skills: skipped"
    return
  fi
  if [ -f "$ROOT/.agents/skills/tdd/SKILL.md" ]; then
    echo "skills: already present under $ROOT/.agents/skills (not overwritten)"
    return
  fi
  if [ -n "${AGENTOS_SKILLS_CMD:-}" ]; then
    (CDPATH= cd -- "$ROOT" && sh -c "$AGENTOS_SKILLS_CMD")
    echo "skills: installed via AGENTOS_SKILLS_CMD"
    return
  fi
  if ! command -v npx >/dev/null 2>&1; then
    echo "error: npx is required to install skills, or pass --skip-skills" >&2
    exit 1
  fi
  echo "skills: npx skills@latest add ${REPO}"
  (CDPATH= cd -- "$ROOT" && npx --yes skills@latest add "$REPO" -s '*' -y --copy)
}

find_cli() {
  if [ -n "${AGENTOS_BIN:-}" ] && [ -x "$AGENTOS_BIN" ]; then
    printf '%s\n' "$AGENTOS_BIN"
    return
  fi
  if command -v agentos >/dev/null 2>&1; then
    command -v agentos
    return
  fi
  if [ -x "${BIN_DIR}/agentos" ]; then
    printf '%s\n' "${BIN_DIR}/agentos"
    return
  fi
  src=${AGENTOS_SRC:-$SRC}
  if [ -x "${src}/target/release/agentos" ]; then
    printf '%s\n' "${src}/target/release/agentos"
    return
  fi
  return 1
}

install_cli_from_release() {
  sh "$HERE/install.sh" --prefix "$PREFIX"
}

build_cli_from_source() {
  src=${AGENTOS_SRC:-}
  if [ -z "$src" ] && [ -f "$SRC/Cargo.toml" ] && [ -d "$SRC/crates/agentos-cli" ]; then
    src=$SRC
  fi
  if ! command -v cargo >/dev/null 2>&1; then
    echo "error: no agentos binary and cargo is not installed." >&2
    echo "Install Rust from https://rustup.rs then re-run, or publish/install a GitHub Release." >&2
    exit 1
  fi
  if [ -z "$src" ] || [ ! -f "$src/Cargo.toml" ]; then
    cache="${AGENTOS_CACHE:-${XDG_CACHE_HOME:-$HOME/.cache}/agentos-src}"
    if [ ! -f "$cache/Cargo.toml" ]; then
      if ! command -v git >/dev/null 2>&1; then
        echo "error: git is required to fetch AgentOS source" >&2
        exit 1
      fi
      echo "cli: cloning ${REPO} into $cache"
      mkdir -p "$(dirname "$cache")"
      git clone --depth 1 "https://github.com/${REPO}.git" "$cache"
    fi
    src=$cache
  fi
  echo "cli: cargo build --release (source $src)"
  (CDPATH= cd -- "$src" && cargo build --release -p agentos-cli)
  if [ ! -x "$src/target/release/agentos" ]; then
    echo "error: cargo build did not produce target/release/agentos" >&2
    exit 1
  fi
  printf '%s\n' "$src/target/release/agentos"
}

ensure_cli() {
  if bin=$(find_cli); then
    echo "cli: using $bin"
  else
    if [ -n "${AGENTOS_RELEASE_BASE:-}" ] || { [ -n "${AGENTOS_VERSION:-}" ] && [ "${AGENTOS_VERSION}" != "latest" ]; }; then
      echo "cli: installing from release"
      install_cli_from_release
      bin="${BIN_DIR}/agentos"
    else
      bin=$(build_cli_from_source)
    fi
  fi
  if [ ! -x "$bin" ]; then
    echo "error: could not find or install agentos" >&2
    exit 1
  fi
  if [ "$bin" != "${BIN_DIR}/agentos" ]; then
    cp "$bin" "${BIN_DIR}/agentos"
    chmod 755 "${BIN_DIR}/agentos"
    echo "cli: installed ${BIN_DIR}/agentos"
  fi
  export PATH="${BIN_DIR}:${PATH}"
  "${BIN_DIR}/agentos" --version
}

run_init() {
  if [ "$SKIP_INIT" -eq 1 ]; then
    echo "init: skipped"
    return
  fi
  echo "init: agentos init --root $ROOT"
  "${BIN_DIR}/agentos" init --root "$ROOT"
  echo "init: agentos doctor --root $ROOT"
  set +e
  "${BIN_DIR}/agentos" doctor --root "$ROOT"
  doctor=$?
  set -e
  if [ "$doctor" -ne 0 ]; then
    echo "doctor exited $doctor (project may still be usable; see output above)" >&2
  fi
}

install_skills
ensure_cli
run_init

echo
echo "Done. Skills live under $ROOT/.agents/skills when installed."
echo "CLI: ${BIN_DIR}/agentos"
echo "Add ${BIN_DIR} to PATH if your shell does not see agentos."
echo "Next, from this project: agentos scan"
echo "No API key is required for local scan, context, plan, or evaluate."
