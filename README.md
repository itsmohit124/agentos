# AgentOS

AgentOS is a local-first, agent-agnostic engineering intelligence layer. It gives coding agents
(Cursor, Claude Code, Codex, or any other) the right knowledge, skills, workflow, feedback and
guardrails for a task — without replacing the agent.

> Status: **early development (foundation)**. The CLI can initialise and check a project; the
> knowledge, context, skills and other engines are not implemented yet.

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
```

Global flags: `--root <PATH>`, `--json`, `-v` / `-vv`, `--log-level <FILTER>`.

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
| 3 | `doctor` found a critical failure |

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
