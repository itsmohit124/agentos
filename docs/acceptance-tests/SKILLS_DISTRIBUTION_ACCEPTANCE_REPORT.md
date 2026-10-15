# Skills.sh distribution acceptance report

Date: 2026-10-15. Host: macOS aarch64. This is packaging work, not a numbered AgentOS phase.
Phases 0–10 stay `APPROVED` with their original dates. No Git commit or push was made.

## Recommended commands

Skills (does not install the CLI):

```bash
npx skills@latest add itsmohit124/agentos
```

Until this branch is pushed, GitHub `main` still lists **10** skills (no `setup-agentos`).
The local workspace lists **11**. Tested command against the workspace:

```bash
npx skills@latest add /path/to/agentos --skill '*' -a cursor -a claude-code -a codex -y --copy
```

Runtime, after a GitHub Release exists:

```bash
sh install/install.sh --version v0.1.0 --prefix "$HOME/.local"
```

Until a release exists (current state):

```bash
cargo build --release
./target/release/agentos --version
```

First run: `/setup-agentos` in an agent that has the skill, or `agentos init` then `agentos doctor`.

## Results

| Check | Status | Evidence |
|---|---|---|
| Public GitHub `npx skills add itsmohit124/agentos -l` | PASS for current `main` | Discovers 10 canonical skills; does not pick up `fixtures/skills` |
| Local workspace `skills add <path> -l` | PASS | Discovers 11 skills including `setup-agentos` |
| Install to Cursor, Claude Code, Codex with `--copy` | PASS as a file copy | Exit 0. Canonical copies in `.agents/skills/<name>/`. Claude copies in `.claude/skills/<name>/`. Codex used the `.agents/skills` tree. `keep.txt` unchanged |
| User-only flag on copies | PASS | Installed `grilling/SKILL.md` still has `disable-model-invocation: true` and `invocation: user` |
| `npx skills remove setup-agentos -y` | PASS | Exit 0; `keep.txt` still `leave this` |
| Live Cursor / Claude / Codex invocation | BLOCKED | Those binaries are not installed |
| `install/tests/run.sh` | PASS | Install, `--version`, uninstall, missing artifact, checksum mismatch, no-release error |
| GitHub Release download of a real binary | BLOCKED | No release/tag artifacts exist |
| `install/install.ps1` | NOT TESTED | Script is present; this host is not Windows |
| Linux aarch64 / Windows arm64 runtime | NOT TESTED | Not in the release matrix as proven builds |
| GitHub Actions packaging and release jobs | NOT TESTED | Workflows were written, not executed |
| `agentos evaluate skills` | PASS | 81/81 after adding `setup-agentos` eval cases. `evals/thresholds.json` skills floor updated to 81 from that measurement |
| `cargo test -p agentos-skills --lib` | PASS | 74 passed |
| `cargo test -p agentos-cli --test skills` | PASS | 20 passed |
| Genuine implementation task with a live agent | BLOCKED | No agent was launched |

## Limits

- `npx skills add` copies markdown. It does not compile or install `agentos`.
- A one-line runtime install is not true until someone publishes `v*` GitHub Release assets and `SHA256SUMS`.
- The skills CLI agent name is `claude-code` and `github-copilot`, not `copilot`. An invalid name fails before install.
- Codex install did not create a separate `.codex/skills` tree in this run; it shared `.agents/skills` with Cursor.

## Files changed

Skills and evals: `skills/setup-agentos/SKILL.md`, `evals/skills/discovery_004.json`, `evals/skills/validate_001.json`, `evals/thresholds.json`.

Plugin and install: `.claude-plugin/plugin.json`, `install/install.sh`, `install/install.ps1`, `install/pack-release.sh`, `install/tests/run.sh`, `install/tests/plugin_layout.sh`.

CI: `.github/workflows/ci.yml` (packaging job), `.github/workflows/release.yml`.

Docs: `README.md`, `docs/INSTALL.md`, `docs/distribution.md`, `docs/user-guide.md`.

Tests: `crates/agentos-skills/src/tests.rs`, `crates/agentos-cli/tests/skills.rs`.

Progress log only: a distribution note. Phase table and approval dates were not changed.
