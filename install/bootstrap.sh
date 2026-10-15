#!/usr/bin/env sh
# Wrapper. The script `npx skills add` copies lives on the setup-agentos skill.
set -eu
HERE=$(CDPATH= cd -- "$(dirname "$0")" && pwd)
exec sh "$HERE/../skills/setup-agentos/scripts/setup.sh" "$@"
