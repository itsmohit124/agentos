#!/usr/bin/env sh
# Local tests for install/bootstrap.sh. Does not call npx or cargo.
set -eu
ROOT=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
BOOTSTRAP="$ROOT/install/bootstrap.sh"
WORKDIR=$(mktemp -d)
trap 'rm -rf "$WORKDIR"' EXIT
# Do not pick up a real ~/.cache/agentos-src binary from this machine.
export AGENTOS_CACHE="$WORKDIR/empty-cache"

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
if PATH="$EMPTY:/usr/bin:/bin" AGENTOS_PREFIX="$WORKDIR/noprefix" AGENTOS_SRC="$WORKDIR/nosrc" \
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

echo "bootstrap tests passed"
