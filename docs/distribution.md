# Distribution

AgentOS ships as portable skills plus a native CLI. Do not treat `npx skills add` as a
runtime installer.

## Skills layout

Canonical skills live in `skills/<name>/SKILL.md`. The Vercel `skills` CLI discovers those
directories. Fixture `SKILL.md` files under `fixtures/skills/` are tests, not a distributable
set.

`.claude-plugin/plugin.json` lists the same directories for Claude Code's plugin installer.
The AgentOS registry still loads built-ins from the `skills/` tree compiled into the binary.

## Runtime artifacts

GitHub Actions `.github/workflows/release.yml` builds:

| Target | Archive |
|---|---|
| `aarch64-apple-darwin` | `.tar.gz` |
| `x86_64-apple-darwin` | `.tar.gz` |
| `x86_64-unknown-linux-gnu` | `.tar.gz` |
| `x86_64-pc-windows-msvc` | `.zip` |

Linux aarch64 and Windows arm64 are not in that matrix. Claiming those platforms would be
untested.

`install/pack-release.sh` writes `dist/SHA256SUMS`. `install/install.sh` refuses to install
when the file is missing or the hash differs.

## Versioning

The CLI version is the workspace version (`0.1.0`). Release tags are `v0.1.0`. Skills keep
their own `version` field in front-matter. Updating skills with `npx skills update` does not
upgrade the binary. Re-running `install.sh --version vX.Y.Z` replaces only `PREFIX/bin/agentos`.
