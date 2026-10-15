# AgentOS

AgentOS is a local-first, agent-agnostic engineering intelligence layer. It gives coding agents
(Cursor, Claude Code, Codex, or any other) the right knowledge, skills, workflow, feedback and
guardrails for a task — without replacing the agent.

> Status: **V1 evaluation**. The CLI can initialise a project, manage portable skills, build a
> derived knowledge graph, assemble a budgeted Context Pack, run validation, plan a workflow, record
> memory, and install an adapter for Cursor, Claude Code, Codex, or a generic agent. `agentos evaluate`
> reports the measured suites. It does not replace the coding agent. AgentOS does not send telemetry.

## Quick start

From the project you want AgentOS to manage:

```bash
sh /path/to/agentos/install/bootstrap.sh
```

That copies skills if they are missing, installs the CLI into `~/.local/bin`, and runs `init`.
It does not overwrite existing skills or `.agentos/config.toml`.

`npx skills@latest add itsmohit124/agentos` is skills only. It does not install the CLI or run `init`.

Until a GitHub Release exists, bootstrap needs Rust (or an `agentos` binary already on PATH).
After a release:

```bash
sh install/install.sh --version v0.1.0 --prefix "$HOME/.local"
```

Supported agents for skills: anything the [skills CLI](https://github.com/vercel-labs/skills)
lists, including Cursor, Claude Code, and Codex. A missing agent binary is not a skills-install
failure. Live Cursor, Claude, or Codex sessions are a separate check.

Runtime: macOS and Linux x86_64/arm64 via the installer when those artifacts exist; Windows via
`install/install.ps1`. Untested targets are not claimed. No API key is required for local
workflows.

Details: [docs/INSTALL.md](docs/INSTALL.md), [docs/distribution.md](docs/distribution.md),
[docs/user-guide.md](docs/user-guide.md).

## Build

Requirements: Rust (stable, edition 2024) and Git. SQLite is bundled; no system install needed.

```bash
cargo build --release
./target/release/agentos --version
```

## Usage

```bash
agentos init              # create .agentos/ (config, profile, derived SQLite index); safe to re-run
agentos doctor            # check root, git, .agentos, config, database, writability
agentos doctor --json     # same, machine-readable
agentos config            # effective configuration and the source of every value

agentos project profile          # classification, maturity, signals (written by init and scan)
agentos scan                     # index the repository; a second run reports 0 changed
agentos knowledge stats          # node, edge and provenance counts
agentos knowledge nodes          # logical ids
agentos knowledge edges
agentos knowledge inspect <id>
agentos knowledge rebuild        # delete the derived graph and scan again

agentos skills list               # skills that won discovery, with level and invocation
agentos skills inspect tdd        # metadata, resolved dependencies, permissions, body
agentos skills inspect tdd --brief  # metadata only; the body is not read
agentos skills validate           # everything, including bodies; exit 3 on errors
agentos skills install ./my-skill # copy a local skill into .agents/skills/ (git URLs: not yet)
agentos skills graph [--format dot]
agentos domain scan               # infer domain nodes; never confirms them
agentos domain contexts
agentos domain terminology
agentos domain map
agentos domain propose            # one lifecycle step toward Proposed
agentos domain confirm <id>       # Proposed → Confirmed, or Confirmed → Canonical
agentos domain reject <id>
agentos domain validate           # exit 3 when canonical evidence has drifted
agentos benchmark baseline fixtures/python-fastapi
                              # naive file-token baseline; writes evals/baselines/context-baseline.json
agentos context explain "Add retry support to payment webhook processing"
agentos context build "Add retry support to payment webhook processing" --format json
agentos context stats "Add retry support to payment webhook processing"
agentos evaluate skills           # run evals/skills; exit 3 if a case fails
agentos evaluate knowledge        # project profiles and language graph fixtures
agentos evaluate ddd              # domain lifecycle fixtures
agentos evaluate context          # context packs against evals/context
agentos validate detect           # providers selected for this project
agentos validate --kind unit_test --json
agentos validate --kind browser --json   # optional; expected/ui.json selects the provider; the CLI still uses NullBrowser
agentos validate last             # most recently stored result
agentos validate record-regression <id>
                              # write evals/feedback/<id>.json from a failed result
agentos architecture analyze     # detectors over the knowledge graph; exit 3 if any finding
agentos evaluate feedback         # detection, parsing, permission, regressions
agentos evaluate architecture     # bad and good architecture fixtures
agentos plan "Add retry support to payment webhook processing" --workflow feature
agentos run <plan-id> --runner mock
agentos run status <run-id>
agentos run resume <run-id>
agentos run abort <run-id>
agentos evaluate workflow         # plans, scheduler, resume, fix loop
agentos memory list
agentos memory show <id>
agentos memory add --type lesson --name "Idempotency" --content "..."
agentos memory confirm <id>
agentos memory forget <id>
agentos memory prune --stale
agentos memory query "Add retry to payment webhook"
agentos evaluate memory           # lessons, staleness, forget, redaction, budget

agentos adapter list
agentos adapter install generic  # also: cursor, claude, codex
agentos adapter doctor generic   # exit 3 when the adapter is not installed or a check fails
agentos adapter uninstall generic
agentos evaluate adapters         # install, uninstall, skills, context, workflow, feedback
agentos evaluate                 # dashboard for every suite; exit 3 if one fails
agentos evaluate --json          # the same dashboard for CI
agentos evaluate e2e             # golden workflows, including a patch that must be rejected

agentos cleanup                  # prints what would be removed and exits 2
agentos cleanup --yes            # removes .agentos/ and managed adapter blocks
```

`adapter install` writes only AgentOS-managed files in the project selected by `--root`. Run it
against the project you want to integrate, not against a checkout you do not mean to modify.
Installing twice does not duplicate the managed section. `adapter uninstall` removes that section
and leaves the rest of a pre-existing file in place. Copies of those pre-existing files are stored
under `.agentos/adapters/originals/`. Install appends `adapters/originals/` to an existing
`.agentos/.gitignore` without rewriting the lines already there. A user-only skill stays
user-only inside AgentOS: generated instructions do not grant permissions and do not mark it
model-invocable. Install also writes the host controls those products document:
`disable-model-invocation: true` in a user-only `.agents/skills/<name>/SKILL.md`, and
`agents/openai.yaml` with `allow_implicit_invocation: false`. Those lines restate the AgentOS
flag. They do not grant tools. Uninstall does not remove them. No Cursor, Claude, or Codex
process is started by install.

Global flags: `--root <PATH>`, `--json`, `-v` / `-vv`, `--log-level <FILTER>`.

### Skills

A skill is a directory with a `SKILL.md`: YAML front-matter (schema:
[`schemas/skill-manifest.schema.json`](schemas/skill-manifest.schema.json)) plus a markdown body.
Discovery locations, highest priority first:

1. `<root>/.agents/skills/`
2. `<root>/.agentos/skills/`
3. user-level: `--user-skills-dir`, else `AGENTOS_USER_SKILLS_DIR`, else `$HOME/.agentos/skills`
4. built-in skills compiled into `agentos` (from `skills/`; replace with `--builtin-skills-dir` /
   `AGENTOS_BUILTIN_SKILLS_DIR`)

A higher level overrides a lower one. Two skills with the same name at the same level are an error,
and a broken skill hides lower-level copies of the same name instead of silently falling back.

Invocation: `user_invocable` (default `true`) and `model_invocable` (default `false`) are separate.
A model-invocable skill may not require, or route to, a skill that is only user-invocable.
Permissions (`fs.read`, `fs.write`, `shell`, `network`, `browser`) are default-deny.

### Project root

`--root` if given, otherwise the Git top-level of the current directory, otherwise the current
directory.

### Configuration

Resolved per key, later layers winning:

1. built-in defaults
2. `.agentos/config.toml`
3. environment: `AGENTOS_LOG` (`log.level`), `AGENTOS_OUTPUT` (`output.format`)
4. CLI flags: `--log-level` / `-v` / `-vv`, `--json`

| Key | Default | Values |
|---|---|---|
| `log.level` | `warn` | a level (`off`, `error`, `warn`, `info`, `debug`, `trace`) or tracing directives such as `agentos_core=debug,warn` |
| `output.format` | `text` | `text`, `json` |
| `telemetry.enabled` | `false` | `true` or `false`. Nothing is transmitted. Env: `AGENTOS_TELEMETRY` |

Logs are written to stderr; command results to stdout.

### Exit codes

| Code | Meaning |
|---|---|
| 0 | success |
| 1 | runtime error |
| 2 | usage error, including `cleanup` without `--yes` |
| 3 | a check failed: `doctor` critical failure, `skills validate` errors, `evaluate` case failures, a workflow run that is `failed` or `blocked`, `adapter doctor` when a check fails, `adapter uninstall` or `cleanup` when a managed file cannot be changed safely |

### Project state

```text
.agentos/
├── config.toml          # project configuration (yours to edit; never overwritten)
├── .gitignore           # keeps derived state out of Git
├── project-profile.json # derived classification and signals; rewritten by init and scan
├── agentos.db           # SQLite knowledge index: derived state, safe to delete and rebuild
├── context-cache/       # derived Context Pack cache
├── feedback/            # derived validation results
└── runs/                # derived plans, resumable runs, and browser artifacts
```

## Development

```bash
scripts/check.sh   # fmt, clippy, tests, agentos evaluate, agentos evaluate e2e
```

See [CONTRIBUTING.md](CONTRIBUTING.md), [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md),
[docs/INSTALL.md](docs/INSTALL.md), and [docs/user-guide.md](docs/user-guide.md). Guides:
[write a skill](docs/guides/write-a-skill.md),
[add a language extractor](docs/guides/add-a-language-extractor.md),
[add an adapter](docs/guides/add-an-adapter.md).

`agentos cleanup --yes` deletes `.agentos/` after removing managed adapter blocks. It leaves
`.agents/skills/` and `agents/openai.yaml` in place, including `disable-model-invocation: true`
and `allow_implicit_invocation: false`.
