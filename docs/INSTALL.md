# Install AgentOS

`npx skills add` copies skill folders, including a setup script. It does not run that
script and does not install the CLI.

```bash
npx skills@latest add itsmohit124/agentos
sh .agents/skills/setup-agentos/scripts/setup.sh
```

The second command installs `agentos` into `~/.local/bin`, adds that directory to your
shell rc (`~/.zshrc`, `~/.bashrc`, or Fish config) if it is not already there, runs
`init`, installs adapters detected from files already in the project (not from the
`npx` agent picker), and runs `scan`. Cursor: `.cursor/` or `.agents/skills/`.
Claude: `.claude/` or `CLAUDE.md`. Codex: `.codex/` or `agents/openai.yaml`.
Always `generic`. Pass `--all-adapters` to install all four. It does not overwrite
existing skills or `.agentos/config.toml`. No API key is required.

`sh setup.sh` cannot change PATH in the terminal that launched it. Open a new terminal
(or `source` your rc) once so `agentos` is on PATH there.

Until a GitHub Release exists, the CLI step needs a local binary or Rust
(`cargo build --release`).

Equivalent, if you already have this repository:

```bash
sh /path/to/agentos/install/bootstrap.sh
```

## Skills only

```bash
npx skills@latest add itsmohit124/agentos
npx skills@latest update
npx skills@latest remove setup-agentos -y
```

Use `-a cursor`, `-a claude-code`, or `-a codex` when you want a specific agent.

User-only skills (`grilling`, `engineering-router`, `improve-codebase-architecture`,
`setup-agentos`) declare `disable-model-invocation: true`. Keep that field.

## CLI only (checksummed GitHub Release)

After a tag `vX.Y.Z` is published:

```bash
sh install/install.sh --version v0.1.0 --prefix "$HOME/.local"
```

Missing artifacts and checksum mismatches exit 1 and do not install. Windows: `install/install.ps1`.

Uninstall the binary only:

```bash
sh install/install.sh --uninstall --prefix "$HOME/.local"
```

That does not delete project `.agentos/` directories or skill files.

## After setup

```bash
export PATH="$HOME/.local/bin:$PATH"
agentos doctor
agentos scan
```

`/setup-agentos` in an agent tells you to run the bundled script if the CLI is missing.
