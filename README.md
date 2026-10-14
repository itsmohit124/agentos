# AgentOS

AgentOS is a local-first, agent-agnostic engineering intelligence layer. It gives coding agents
(Cursor, Claude Code, Codex, or any other) the right knowledge, skills, workflow, feedback and
guardrails for a task — without replacing the agent.

> Status: **early development**. The CLI can initialise a project, manage portable skills, build a
> derived knowledge graph, assemble a budgeted Context Pack, and run validation providers. Workflow
> and the other engines are not implemented yet.

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
```

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

Logs are written to stderr; command results to stdout.

### Exit codes

| Code | Meaning |
|---|---|
| 0 | success |
| 1 | runtime error |
| 2 | usage error |
| 3 | a check failed: `doctor` critical failure, `skills validate` errors, `evaluate` case failures, a workflow run that is `failed` or `blocked` |

### Project state

```text
.agentos/
├── config.toml          # project configuration (yours to edit; never overwritten)
├── .gitignore           # keeps derived state out of Git
├── project-profile.json # derived classification and signals; rewritten by init and scan
├── agentos.db           # SQLite knowledge index: derived state, safe to delete and rebuild
├── context-cache/       # derived Context Pack cache
├── feedback/            # derived validation results
└── runs/                # derived plans and resumable runs
```

## Development

```bash
scripts/check.sh   # cargo fmt --check, clippy -D warnings, cargo test --workspace
```

See [CONTRIBUTING.md](CONTRIBUTING.md) and [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).
