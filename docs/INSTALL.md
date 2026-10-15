# Install AgentOS

From the project you want to analyze, one command installs skills (if they are missing),
ensures the CLI, and runs `init`:

```bash
# If you already have this repository:
sh /path/to/agentos/install/bootstrap.sh

# After this file is on GitHub main:
curl -fsSL https://raw.githubusercontent.com/itsmohit124/agentos/main/install/bootstrap.sh | sh
```

That is the first-run path. It does not overwrite existing `.agents/skills` or
`.agentos/config.toml`. No API key is required.

Until a GitHub Release exists, the CLI step needs a local binary or Rust (`cargo build --release`).
The skills-only command below does **not** run `init` and does **not** install the CLI.

## Skills only (editable copies)

```bash
npx skills@latest add itsmohit124/agentos
```

That copies `SKILL.md` files for Cursor and other supported agents. Use `-a cursor`,
`-a claude-code`, or `-a codex` when you want a specific agent.

```bash
npx skills@latest update
npx skills@latest remove setup-agentos -y
```

User-only skills (`grilling`, `engineering-router`, `improve-codebase-architecture`,
`setup-agentos`) declare `disable-model-invocation: true`. Keep that field.

## CLI only (checksummed GitHub Release)

After a tag `vX.Y.Z` is published:

```bash
sh install/install.sh --version v0.1.0 --prefix "$HOME/.local"
```

Missing artifacts and checksum mismatches exit 1 and do not install. Windows: `install/install.ps1`.

Source build:

```bash
cargo build --release
./target/release/agentos --version
```

Uninstall the binary only:

```bash
sh install/install.sh --uninstall --prefix "$HOME/.local"
```

That does not delete project `.agentos/` directories or skill files.

## After bootstrap

```bash
agentos doctor
agentos scan
```

Add `$HOME/.local/bin` to PATH if `agentos` is not found. `/setup-agentos` in an agent is the
same first-run procedure when the CLI is already on PATH.
