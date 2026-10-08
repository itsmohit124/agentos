# AgentOS

AgentOS is a local-first, agent-agnostic engineering intelligence layer. It gives coding agents
(Cursor, Claude Code, Codex, or any other) the right knowledge, skills, workflow, feedback and
guardrails for a task — without replacing the agent.

> Status: **early development**. The CLI can initialise and check a project and manage portable
> skills; the knowledge, context, workflow and other engines are not implemented yet.

## Build

Requirements: Rust (stable, edition 2024) and Git. SQLite is bundled; no system install needed.

```bash
cargo build --release
./target/release/agentos --version
```

## Usage

```bash
agentos init              # create .agentos/ (config, derived SQLite index); safe to re-run
agentos doctor            # check root, git, .agentos, config, database, writability
agentos doctor --json     # same, machine-readable
agentos config            # effective configuration and the source of every value

agentos skills list               # skills that won discovery, with level and invocation
agentos skills inspect tdd        # metadata, resolved dependencies, permissions, body
agentos skills inspect tdd --brief  # metadata only; the body is not read
agentos skills validate           # everything, including bodies; exit 3 on errors
agentos skills install ./my-skill # copy a local skill into .agents/skills/ (git URLs: not yet)
agentos skills graph [--format dot]
agentos evaluate skills           # run evals/skills; exit 3 if a case fails
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
| 3 | a check failed: `doctor` critical failure, `skills validate` errors, `evaluate` case failures |

### Project state

```text
.agentos/
├── config.toml   # project configuration (yours to edit; never overwritten)
├── .gitignore    # keeps derived state out of Git
└── agentos.db    # SQLite index: derived state, safe to delete and rebuild
```

## Development

```bash
scripts/check.sh   # cargo fmt --check, clippy -D warnings, cargo test --workspace
```

See [CONTRIBUTING.md](CONTRIBUTING.md) and [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).
