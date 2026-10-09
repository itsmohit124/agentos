# Contributing

## Ground rules

- Keep changes focused; don't refactor unrelated code in a feature change.
- Tests first where practical; every bug gets a regression test.
- Core crates stay agent-agnostic and language-agnostic.
- Git and the filesystem are the source of truth; the SQLite index is derived and rebuildable.

## Prerequisites

- Rust stable with `rustfmt` and `clippy`
- Git on `PATH` (tests create temporary repositories)

## Definition of green

```bash
scripts/check.sh
```

which runs:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --workspace
```

## Code style

- Libraries use typed errors (`thiserror`); only the CLI binary uses `anyhow`.
- Log with `tracing`. Only `crates/agentos-cli/src/output.rs` writes to stdout/stderr.
- Sort collections before emitting; no timestamps in golden outputs.
- Document every public item (`missing_docs` is enforced through clippy `-D warnings`).
- Prefer deep modules: define the trait and its contract tests before the implementation.

## Tests and fixtures

- Unit tests live next to the code; CLI end-to-end tests in `crates/agentos-cli/tests/`.
- `agentos-testkit` (dev-dependency only) provides `TempProject`, `CliRunner` and the fixture
  loader (`fixture_path`, `copy_fixture`).
- Fixtures live in `fixtures/<name>/` with ground truth in `expected/` (see `fixtures/README.md`).
- Evaluation cases live in `evals/<area>/` (see `evals/README.md`); run them with
  `agentos evaluate <area>`.
- Built-in skills live in `skills/<name>/SKILL.md` and are compiled into the binary. Each needs the
  body sections `When to use`, `Inputs`, `Procedure` (numbered), `Outputs`, `Must not`, `Validation`
  and at least one `eval_cases` entry; `agentos skills validate` fails otherwise. When a router case
  fails, tune the skill's `triggers`, never the expected answer.

## Database migrations

Migrations for the core `meta` table are listed in `crates/agentos-core/src/db.rs` (`MIGRATIONS`).
Append new core migrations with the next version number; never edit or reorder a released migration.
The knowledge graph uses its own migration table inside the same `agentos.db` (`agentos-knowledge`),
because `agentos-core` cannot depend on that crate.
