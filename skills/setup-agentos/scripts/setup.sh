#!/usr/bin/env sh
# First-run: ensure the CLI, then init this project.
# Copied by `npx skills add` into .agents/skills/setup-agentos/scripts/.
# npx does not run this file; you (or /setup-agentos) do.
# Usage: sh setup.sh [--root DIR] [--prefix DIR] [--skip-skills] [--skip-init] [--skip-adapters] [--skip-path]
set -eu

HERE=$(CDPATH= cd -- "$(dirname "$0")" && pwd)
SRC=""
ROOT="${AGENTOS_ROOT:-.}"
PREFIX="${AGENTOS_PREFIX:-${HOME}/.local}"
SKIP_SKILLS=0
SKIP_INIT=0
SKIP_ADAPTERS=0
SKIP_PATH=0
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
    --skip-adapters)
      SKIP_ADAPTERS=1
      shift
      ;;
    --skip-path)
      SKIP_PATH=1
      shift
      ;;
    -h|--help)
      cat <<'EOF'
Set up the AgentOS CLI and initialize the current project.

  sh .agents/skills/setup-agentos/scripts/setup.sh

`npx skills add` copies this script. It does not execute it.

CLI resolution order:
  1. AGENTOS_BIN, or agentos already on PATH, or PREFIX/bin/agentos
  2. A release binary via install-cli.sh (checksummed)
  3. cargo build --release in AGENTOS_SRC, this AgentOS repo, or a cache clone

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

looks_like_src() {
  [ -f "$1/Cargo.toml" ] && [ -d "$1/crates/agentos-cli" ]
}

# Canonical tree: skills/setup-agentos/scripts. Copied tree: .agents/skills/setup-agentos/scripts
# (four parents is the project, which may itself be AgentOS).
# If AGENTOS_SRC is set, do not guess another tree (tests and explicit overrides).
if [ -n "${AGENTOS_SRC:-}" ]; then
  SRC=$AGENTOS_SRC
elif looks_like_src "$ROOT"; then
  SRC=$ROOT
elif looks_like_src "$HERE/../../.."; then
  SRC=$(CDPATH= cd -- "$HERE/../../.." && pwd)
elif looks_like_src "$HERE/../../../.."; then
  SRC=$(CDPATH= cd -- "$HERE/../../../.." && pwd)
fi

echo "project: $ROOT" >&2
if [ -n "$SRC" ]; then
  echo "source: $SRC" >&2
fi

install_skills() {
  if [ "$SKIP_SKILLS" -eq 1 ]; then
    echo "skills: skipped" >&2
    return
  fi
  if [ -f "$ROOT/.agents/skills/tdd/SKILL.md" ]; then
    echo "skills: already present under $ROOT/.agents/skills (not overwritten)" >&2
    return
  fi
  if [ -n "${AGENTOS_SKILLS_CMD:-}" ]; then
    (CDPATH= cd -- "$ROOT" && sh -c "$AGENTOS_SKILLS_CMD")
    echo "skills: installed via AGENTOS_SKILLS_CMD" >&2
    return
  fi
  if ! command -v npx >/dev/null 2>&1; then
    echo "error: npx is required to install remaining skills, or pass --skip-skills" >&2
    exit 1
  fi
  echo "skills: npx skills@latest add ${REPO}" >&2
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
  built=$(release_bin "$src")
  if [ -n "$built" ]; then
    printf '%s\n' "$built"
    return
  fi
  cache="${AGENTOS_CACHE:-${XDG_CACHE_HOME:-$HOME/.cache}/agentos-src}"
  built=$(release_bin "$cache")
  if [ -n "$built" ]; then
    printf '%s\n' "$built"
    return
  fi
  return 1
}

release_bin() {
  src=$1
  [ -n "$src" ] || return 1
  if [ -n "${CARGO_TARGET_DIR:-}" ]; then
    case "${CARGO_TARGET_DIR}" in
      /*) ;;
      *)
        if [ -x "${src}/${CARGO_TARGET_DIR}/release/agentos" ]; then
          printf '%s\n' "${src}/${CARGO_TARGET_DIR}/release/agentos"
          return
        fi
        ;;
    esac
  fi
  if [ -x "${src}/target/release/agentos" ]; then
    printf '%s\n' "${src}/target/release/agentos"
    return
  fi
  return 1
}

install_cli_from_release() {
  sh "$HERE/install-cli.sh" --prefix "$PREFIX"
}

build_cli_from_source() {
  src=${AGENTOS_SRC:-$SRC}
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
      echo "cli: cloning ${REPO} into $cache" >&2
      mkdir -p "$(dirname "$cache")"
      git clone --depth 1 "https://github.com/${REPO}.git" "$cache"
    fi
    src=$cache
  fi
  echo "cli: cargo build --release (source $src)" >&2
  (CDPATH= cd -- "$src" && CARGO_TARGET_DIR="$src/target" cargo build --release -p agentos-cli >&2)
  built=$(release_bin "$src" || true)
  if [ -z "$built" ]; then
    echo "error: cargo build did not produce a release agentos binary" >&2
    exit 1
  fi
  printf '%s\n' "$built"
}

ensure_cli() {
  if bin=$(find_cli); then
    echo "cli: using $bin" >&2
  else
    if [ -n "${AGENTOS_RELEASE_BASE:-}" ] || { [ -n "${AGENTOS_VERSION:-}" ] && [ "${AGENTOS_VERSION}" != "latest" ]; }; then
      echo "cli: installing from release" >&2
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
    echo "cli: installed ${BIN_DIR}/agentos" >&2
  fi
  export PATH="${BIN_DIR}:${PATH}"
  "${BIN_DIR}/agentos" --version
}

# Append PREFIX/bin to the user's shell rc so later terminals see `agentos`.
# A child `sh setup.sh` cannot change PATH in the parent shell.
persist_path() {
  if [ "$SKIP_PATH" -eq 1 ] || [ -n "${AGENTOS_SKIP_PATH:-}" ]; then
    echo "path: skipped" >&2
    return
  fi
  line="export PATH=\"${BIN_DIR}:\$PATH\""
  rc=${AGENTOS_SHELL_RC:-}
  if [ -z "$rc" ]; then
    shell_name=${SHELL:-/bin/sh}
    shell_name=${shell_name##*/}
    case "$shell_name" in
      zsh) rc="${HOME}/.zshrc" ;;
      bash) rc="${HOME}/.bashrc" ;;
      fish) rc="${HOME}/.config/fish/config.fish" ;;
      *) rc="${HOME}/.profile" ;;
    esac
  fi
  mkdir -p "$(dirname "$rc")"
  if [ -f "$rc" ] && grep -q "agentos:path-begin" "$rc"; then
    echo "path: already configured in $rc" >&2
    return
  fi
  if [ ! -f "$rc" ]; then
    touch "$rc"
  fi
  begin="# agentos:path-begin"
  end="# agentos:path-end"
  if [ "${rc##*/}" = "config.fish" ]; then
    block="${begin}
# AgentOS CLI — managed by setup.sh; do not edit this block
fish_add_path -m ${BIN_DIR}
${end}"
  else
    block="${begin}
# AgentOS CLI — managed by setup.sh; do not edit this block
${line}
${end}"
  fi
  printf '\n%s\n' "$block" >> "$rc"
  echo "path: added ${BIN_DIR} to $rc" >&2
}

run_init() {
  if [ "$SKIP_INIT" -eq 1 ]; then
    echo "init: skipped" >&2
    return
  fi
  echo "init: agentos init --root $ROOT" >&2
  "${BIN_DIR}/agentos" init --root "$ROOT"
  echo "init: agentos doctor --root $ROOT" >&2
  set +e
  "${BIN_DIR}/agentos" doctor --root "$ROOT"
  doctor=$?
  set -e
  if [ "$doctor" -ne 0 ]; then
    echo "doctor exited $doctor (project may still be usable; see output above)" >&2
  fi
}

install_adapters() {
  if [ "$SKIP_ADAPTERS" -eq 1 ]; then
    echo "adapters: skipped" >&2
    return
  fi
  # Every adapter writes a different managed path. A normal prompt in that agent
  # only follows AgentOS after the adapter is installed.
  for adapter in generic cursor claude codex; do
    echo "adapters: agentos adapter install ${adapter}" >&2
    "${BIN_DIR}/agentos" adapter install "$adapter" --root "$ROOT"
  done
  echo "scan: agentos scan --root $ROOT" >&2
  set +e
  "${BIN_DIR}/agentos" scan --root "$ROOT"
  scan=$?
  set -e
  if [ "$scan" -ne 0 ]; then
    echo "scan exited $scan (you can re-run: agentos scan)" >&2
  fi
}

install_skills
ensure_cli
persist_path
run_init
install_adapters

echo
echo "Done. Skills live under $ROOT/.agents/skills when installed."
echo "CLI: ${BIN_DIR}/agentos"
echo "PATH includes ${BIN_DIR} in this setup process and in your shell rc for new terminals."
echo "This terminal: export PATH=\"${BIN_DIR}:\$PATH\"   (or open a new terminal)"
echo "No API key is required for local scan, context, plan, or evaluate."
