# User guide

AgentOS is a local tool. It indexes the project, selects context for a task, and checks the
result. It does not call a model and it does not send telemetry.

## Start

See [INSTALL.md](INSTALL.md).

```bash
npx skills@latest add itsmohit124/agentos
setup=.agents/skills/setup-agentos/scripts/setup.sh
[ -f "$setup" ] || setup="$HOME/.agents/skills/setup-agentos/scripts/setup.sh"
sh "$setup"
agentos doctor
```

After a global install, the skills live in `~/.agents/skills`. Inside each repo you
want indexed, run:

```bash
cd /path/to/the/repo
sh ~/.agents/skills/setup-agentos/scripts/setup.sh
agentos doctor
agentos context build "the change you want in this repo"
```

That creates this repo's `.agentos/` index, copies the skills into `.agents/skills/`,
and installs the Cursor hook that attaches this repo's Context Pack. The next repo
gets its own `.agentos/` when you run the same command there. Do not run it from
your home directory.

`--root` selects the project. Without it, AgentOS uses the Git root of the current directory.

## Understand the project

```bash
agentos scan
agentos knowledge stats
agentos domain scan
agentos context build "Add retry support to payment webhook processing"
```

`scan` rebuilds the derived graph from the files. `context build` returns one Context Pack and a
retrieval trace. The same task produces the same pack.

## Plan and check

```bash
agentos plan "Add retry support to payment webhook processing"
agentos validate --kind unit_test
agentos architecture analyze
```

`plan` selects the feature, bugfix, or refactor workflow and expands skill dependencies. It does
not edit the tree. `validate` runs a detected provider only when the `shell` capability is granted.
`architecture analyze` reads the graph that `scan` already built.

## Evaluate

```bash
agentos evaluate
agentos evaluate --json
agentos evaluate context
agentos evaluate e2e
```

The dashboard covers knowledge, DDD, context, skills, workflow, feedback, architecture, memory, and
adapters. `evaluate e2e` runs the golden payment-webhook workflow on `fixtures/python-fastapi` and
`fixtures/node-express`. A patch that breaks the aggregate or edits an unrelated file fails that
workflow. Exit code 3 means a case failed.

## Adapters and cleanup

```bash
agentos adapter install generic
agentos adapter doctor generic
agentos cleanup
agentos cleanup --yes
```

Install writes managed blocks only. A user-only skill stays user-only. Cleanup without `--yes`
changes nothing and exits 2. `cleanup --yes` uninstalls managed adapter blocks and deletes
`.agentos/`. It does not delete `.agents/skills/` or `agents/openai.yaml`.

## Configuration

`.agentos/config.toml` overrides defaults. `AGENTOS_LOG`, `AGENTOS_OUTPUT`, and `AGENTOS_TELEMETRY`
override the file. `telemetry.enabled` defaults to false. Setting it to true does not transmit
anything: this build has no telemetry client.

## Exit codes

| Code | Meaning |
|---|---|
| 0 | success |
| 1 | runtime error |
| 2 | usage, including cleanup without `--yes` |
| 3 | a check failed |
