#!/usr/bin/env sh
# The plugin manifest must list every canonical skill directory exactly once.
set -eu
ROOT=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
plugin="$ROOT/.claude-plugin/plugin.json"
[ -f "$plugin" ] || {
  echo "missing $plugin" >&2
  exit 1
}

listed=$(python3 - <<PY
import json
from pathlib import Path
plugin = Path("$plugin")
data = json.loads(plugin.read_text())
paths = sorted(p.removeprefix("./") for p in data["skills"])
print("\n".join(paths))
PY
)

found=$(find "$ROOT/skills" -mindepth 2 -maxdepth 2 -name SKILL.md | sed "s|$ROOT/||; s|/SKILL.md||" | sort)
if [ "$listed" != "$found" ]; then
  echo "plugin.json skills do not match skills/*/SKILL.md" >&2
  echo "listed:" >&2
  echo "$listed" >&2
  echo "found:" >&2
  echo "$found" >&2
  exit 1
fi
[ -f "$ROOT/skills/setup-agentos/scripts/setup.sh" ] || {
  echo "missing skills/setup-agentos/scripts/setup.sh" >&2
  exit 1
}
[ -f "$ROOT/skills/setup-agentos/scripts/install-cli.sh" ] || {
  echo "missing skills/setup-agentos/scripts/install-cli.sh" >&2
  exit 1
}

echo "plugin layout ok"
