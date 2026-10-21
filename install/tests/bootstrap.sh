#!/usr/bin/env sh
# Local tests for install/bootstrap.sh. Does not call npx or cargo.
set -eu
ROOT=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
BOOTSTRAP="$ROOT/install/bootstrap.sh"
WORKDIR=$(mktemp -d)
trap 'rm -rf "$WORKDIR"' EXIT
# Do not pick up a real ~/.cache/agentos-src binary from this machine.
export AGENTOS_CACHE="$WORKDIR/empty-cache"
# Do not append PATH to the developer machine's ~/.zshrc.
export AGENTOS_SKIP_PATH=1

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

FAKE="$WORKDIR/fake-agentos"
cat >"$FAKE" <<'EOF'
#!/bin/sh
cmd=$1
shift || true
root=.
while [ $# -gt 0 ]; do
  case "$1" in
    --root) root=$2; shift 2 ;;
    --version) echo "agentos 0.1.0-test"; exit 0 ;;
    *) shift ;;
  esac
done
case "$cmd" in
  --version) echo "agentos 0.1.0-test" ;;
  init)
    mkdir -p "$root/.agentos"
    if [ ! -f "$root/.agentos/config.toml" ]; then
      echo "# test" > "$root/.agentos/config.toml"
    fi
    echo "initialized $root"
    ;;
  doctor)
    echo "doctor ok"
    exit 0
    ;;
  adapter)
    echo "adapter $*"
    if [ -n "${AGENTOS_ADAPTER_LOG:-}" ]; then
      echo "$2" >> "$AGENTOS_ADAPTER_LOG"
    fi
    ;;
  scan)
    echo "scanned $root"
    ;;
  *)
    echo "unexpected $cmd" >&2
    exit 2
    ;;
esac
EOF
chmod 755 "$FAKE"

PROJ="$WORKDIR/proj"
mkdir -p "$PROJ"
PREFIX="$WORKDIR/prefix"

# Skills already present: do not run npx (would fail in this env).
mkdir -p "$PROJ/.agents/skills/tdd"
echo "keep-body" > "$PROJ/.agents/skills/tdd/SKILL.md"
echo "leave-me" > "$PROJ/keep.txt"

AGENTOS_BIN="$FAKE" AGENTOS_PREFIX="$PREFIX" \
  sh "$BOOTSTRAP" --root "$PROJ"

[ -x "$PREFIX/bin/agentos" ] || fail "cli not copied to prefix"
[ -f "$PROJ/.agentos/config.toml" ] || fail "init did not create config"
[ "$(cat "$PROJ/keep.txt")" = "leave-me" ] || fail "user file overwritten"
[ "$(cat "$PROJ/.agents/skills/tdd/SKILL.md")" = "keep-body" ] || fail "skill overwritten"

# Existing config is not replaced.
echo "user-config" > "$PROJ/.agentos/config.toml"
AGENTOS_BIN="$FAKE" AGENTOS_PREFIX="$PREFIX" \
  sh "$BOOTSTRAP" --root "$PROJ"
[ "$(cat "$PROJ/.agentos/config.toml")" = "user-config" ] || fail "config overwritten"

# Missing CLI and no cargo on a PATH that excludes cargo: clear error.
# We still have AGENTOS_BIN unset; hide cargo and the copied prefix binary.
EMPTY="$WORKDIR/empty-bin"
mkdir -p "$EMPTY"
PROJ2="$WORKDIR/proj2"
mkdir -p "$PROJ2"
if PATH="$EMPTY:/usr/bin:/bin" AGENTOS_SKIP_RUSTUP=1 AGENTOS_PREFIX="$WORKDIR/noprefix" AGENTOS_SRC="$WORKDIR/nosrc" \
  sh "$BOOTSTRAP" --root "$PROJ2" --skip-skills 2>"$WORKDIR/nocli.err"; then
  fail "missing cli should fail"
fi
grep -q "cargo is not installed" "$WORKDIR/nocli.err" || grep -q "no agentos binary" "$WORKDIR/nocli.err" \
  || fail "missing cli error was unclear"

# Skills command is invoked when skills are absent.
PROJ3="$WORKDIR/proj3"
mkdir -p "$PROJ3"
MARKER="$WORKDIR/skills-ran"
AGENTOS_BIN="$FAKE" AGENTOS_PREFIX="$PREFIX" \
  AGENTOS_SKILLS_CMD="touch '$MARKER'" \
  sh "$BOOTSTRAP" --root "$PROJ3"
[ -f "$MARKER" ] || fail "skills command was not run"

# Copied skill layout (what `npx skills add` leaves in a project).
COPIED="$WORKDIR/copied/.agents/skills/setup-agentos"
mkdir -p "$COPIED/scripts" "$WORKDIR/copied/.agents/skills/tdd"
cp "$ROOT/skills/setup-agentos/scripts/setup.sh" "$COPIED/scripts/"
cp "$ROOT/skills/setup-agentos/scripts/install-cli.sh" "$COPIED/scripts/"
echo "keep-body" > "$WORKDIR/copied/.agents/skills/tdd/SKILL.md"
AGENTOS_BIN="$FAKE" AGENTOS_PREFIX="$PREFIX" \
  sh "$COPIED/scripts/setup.sh" --root "$WORKDIR/copied"
[ -f "$WORKDIR/copied/.agentos/config.toml" ] || fail "copied skill setup did not init"

# Global install: script lives in ~/.agents/skills. Copy skills into the project.
# Do not call npx (PromptScript and the scope prompt must not run again).
GHOME="$WORKDIR/global-home"
GSKILLS="$GHOME/.agents/skills"
mkdir -p "$GSKILLS/setup-agentos/scripts" "$GSKILLS/tdd" "$GSKILLS/research"
cp "$ROOT/skills/setup-agentos/scripts/setup.sh" "$GSKILLS/setup-agentos/scripts/"
cp "$ROOT/skills/setup-agentos/scripts/install-cli.sh" "$GSKILLS/setup-agentos/scripts/"
cp "$ROOT/skills/setup-agentos/SKILL.md" "$GSKILLS/setup-agentos/SKILL.md"
printf '%s\n' '---' 'name: tdd' 'description: Test skill.' 'version: "0.1.0"' '---' 'global-tdd' > "$GSKILLS/tdd/SKILL.md"
printf '%s\n' '---' 'name: research' 'description: Test skill.' 'version: "0.1.0"' '---' 'global-research' > "$GSKILLS/research/SKILL.md"
mkdir -p "$GSKILLS/codebase-design"
printf '%s\n' '---' 'name: codebase-design' 'description: Host skill.' '---' 'host-skill' > "$GSKILLS/codebase-design/SKILL.md"
GPROJ="$WORKDIR/global-proj"
mkdir -p "$GPROJ"
GMARKER="$WORKDIR/global-npx"
HOME="$GHOME" AGENTOS_BIN="$FAKE" AGENTOS_PREFIX="$PREFIX" \
  AGENTOS_SKILLS_CMD="touch '$GMARKER'" \
  sh "$GSKILLS/setup-agentos/scripts/setup.sh" --root "$GPROJ"
[ -f "$GPROJ/.agentos/config.toml" ] || fail "global setup did not init"
grep -q "global-tdd" "$GPROJ/.agents/skills/tdd/SKILL.md" || fail "global skills not copied"
grep -q 'version: "0.1.0"' "$GPROJ/.agents/skills/tdd/SKILL.md" || fail "agentos skill front matter dropped"
[ -f "$GPROJ/.agents/skills/research/SKILL.md" ] || fail "research skill not copied"
[ ! -e "$GPROJ/.agents/skills/codebase-design" ] || fail "host skill without version was copied"
[ -f "$GPROJ/.agents/skills/setup-agentos/scripts/setup.sh" ] || fail "setup skill not copied"
[ ! -f "$GMARKER" ] || fail "global setup invoked the skills installer"
echo "edited" > "$GPROJ/.agents/skills/tdd/SKILL.md"
HOME="$GHOME" AGENTOS_BIN="$FAKE" AGENTOS_PREFIX="$PREFIX" \
  sh "$GSKILLS/setup-agentos/scripts/setup.sh" --root "$GPROJ"
[ "$(cat "$GPROJ/.agents/skills/tdd/SKILL.md")" = "edited" ] || fail "copied skill overwritten"

# The home directory is not a project. init would walk protected folders and fail.
HOME_AS_ROOT="$WORKDIR/home-as-root"
mkdir -p "$HOME_AS_ROOT"
if HOME="$HOME_AS_ROOT" AGENTOS_BIN="$FAKE" AGENTOS_PREFIX="$PREFIX" \
  sh "$BOOTSTRAP" --root "$HOME_AS_ROOT" --skip-skills 2>"$WORKDIR/home.err"; then
  fail "home root should fail"
fi
grep -q "home directory" "$WORKDIR/home.err" || fail "home root error was unclear"
[ ! -d "$HOME_AS_ROOT/.agentos" ] || fail "home root was initialized"

# cargo prints to stdout; that must not become the installed path.
FAKESRC="$WORKDIR/fakesrc"
mkdir -p "$FAKESRC/crates/agentos-cli" "$WORKDIR/fake-cargo"
echo '[workspace]' > "$FAKESRC/Cargo.toml"
cat >"$WORKDIR/fake-cargo/cargo" <<'EOF'
#!/bin/sh
echo "Compiling agentos-cli"
mkdir -p target/release
printf '#!/bin/sh\necho agentos 0.1.0-test\n' > target/release/agentos
chmod 755 target/release/agentos
EOF
chmod 755 "$WORKDIR/fake-cargo/cargo"
PROJ4="$WORKDIR/proj4"
mkdir -p "$PROJ4"
PREFIX4="$WORKDIR/prefix4"
PATH="$WORKDIR/fake-cargo:/usr/bin:/bin" AGENTOS_SRC="$FAKESRC" AGENTOS_PREFIX="$PREFIX4" \
  sh "$BOOTSTRAP" --root "$PROJ4" --skip-skills
[ -x "$PREFIX4/bin/agentos" ] || fail "noisy cargo did not install the binary"
out=$("$PREFIX4/bin/agentos" --version)
[ "$out" = "agentos 0.1.0-test" ] || fail "installed binary was $out"

# PATH persistence: append once, keep existing rc text, skip a second time.
RC="$WORKDIR/home/.zshrc"
mkdir -p "$WORKDIR/home"
echo "keep-rc" > "$RC"
PROJ5="$WORKDIR/proj5"
mkdir -p "$PROJ5/.agents/skills/tdd"
echo x > "$PROJ5/.agents/skills/tdd/SKILL.md"
env -u AGENTOS_SKIP_PATH HOME="$WORKDIR/home" AGENTOS_SHELL_RC="$RC" \
  AGENTOS_BIN="$FAKE" AGENTOS_PREFIX="$PREFIX" \
  sh "$BOOTSTRAP" --root "$PROJ5"
grep -q "keep-rc" "$RC" || fail "shell rc overwritten"
grep -q "agentos:path-begin" "$RC" || fail "PATH block missing"
grep -q "$PREFIX/bin" "$RC" || fail "bin dir not on PATH in rc"
before=$(wc -l < "$RC")
env -u AGENTOS_SKIP_PATH HOME="$WORKDIR/home" AGENTOS_SHELL_RC="$RC" \
  AGENTOS_BIN="$FAKE" AGENTOS_PREFIX="$PREFIX" \
  sh "$BOOTSTRAP" --root "$PROJ5"
after=$(wc -l < "$RC")
[ "$before" -eq "$after" ] || fail "PATH block duplicated"

# No cargo on PATH and no prebuilt binary: setup installs Rust (fake rustup), then builds.
HOME_RUST="$WORKDIR/rust-home"
mkdir -p "$HOME_RUST"
FAKESRC2="$WORKDIR/fakesrc-norust"
mkdir -p "$FAKESRC2/crates/agentos-cli"
echo '[workspace]' > "$FAKESRC2/Cargo.toml"
RUST_MARK="$WORKDIR/rustup-ran"
PROJ6="$WORKDIR/proj6"
mkdir -p "$PROJ6"
PREFIX6="$WORKDIR/prefix6"
PATH="$EMPTY:/usr/bin:/bin" HOME="$HOME_RUST" AGENTOS_SRC="$FAKESRC2" AGENTOS_PREFIX="$PREFIX6" \
  AGENTOS_RUSTUP_CMD="mkdir -p \"\$HOME/.cargo/bin\" && cp '$WORKDIR/fake-cargo/cargo' \"\$HOME/.cargo/bin/cargo\" && chmod 755 \"\$HOME/.cargo/bin/cargo\" && touch '$RUST_MARK'" \
  sh "$BOOTSTRAP" --root "$PROJ6" --skip-skills
[ -f "$RUST_MARK" ] || fail "rustup was not run when cargo was missing"
[ -x "$PREFIX6/bin/agentos" ] || fail "setup did not build after installing rust"
out=$("$PREFIX6/bin/agentos" --version)
[ "$out" = "agentos 0.1.0-test" ] || fail "post-rustup binary was $out"

# rustup's success text is stdout. It must not be captured as the agentos path.
HOME_RUST2="$WORKDIR/rust-home-2"
mkdir -p "$HOME_RUST2"
FAKESRC3="$WORKDIR/fakesrc-norust-2"
mkdir -p "$FAKESRC3/crates/agentos-cli"
echo '[workspace]' > "$FAKESRC3/Cargo.toml"
PROJ7="$WORKDIR/proj7"
mkdir -p "$PROJ7"
PREFIX7="$WORKDIR/prefix7"
PATH="$EMPTY:/usr/bin:/bin" HOME="$HOME_RUST2" AGENTOS_SRC="$FAKESRC3" AGENTOS_PREFIX="$PREFIX7" \
  AGENTOS_RUSTUP_CMD="echo 'Rust is installed now. Great!' && mkdir -p \"\$HOME/.cargo/bin\" && cp '$WORKDIR/fake-cargo/cargo' \"\$HOME/.cargo/bin/cargo\" && chmod 755 \"\$HOME/.cargo/bin/cargo\"" \
  sh "$BOOTSTRAP" --root "$PROJ7" --skip-skills >/dev/null
[ -x "$PREFIX7/bin/agentos" ] || fail "rustup stdout replaced the agentos path"
out=$("$PREFIX7/bin/agentos" --version)
[ "$out" = "agentos 0.1.0-test" ] || fail "rustup stdout binary was $out"

echo "bootstrap tests passed"
