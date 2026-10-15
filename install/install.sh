#!/usr/bin/env sh
# Wrapper. Canonical checksummed installer ships in the setup-agentos skill.
set -eu
HERE=$(CDPATH= cd -- "$(dirname "$0")" && pwd)
exec sh "$HERE/../skills/setup-agentos/scripts/install-cli.sh" "$@"
