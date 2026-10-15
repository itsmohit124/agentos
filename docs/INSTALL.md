# Install AgentOS

There are two installs. They are independent.

1. **Skills** — `SKILL.md` files that coding agents can load.
2. **Runtime** — the `agentos` binary. The skills installer does not compile or install it.

## Skills (editable copies)

```bash
npx skills@latest add itsmohit124/agentos
```

Pick one or more skills, or all of them. Target Cursor, Claude Code, Codex, or another
supported agent with `-a`:

```bash
npx skills@latest add itsmohit124/agentos -a cursor -s '*' -y --copy
npx skills@latest add itsmohit124/agentos -s tdd -s setup-agentos -a claude-code -y --copy
npx skills@latest update
npx skills@latest remove setup-agentos -y
```

`--copy` writes files instead of symlinks. The installer does not overwrite files it does not
own; if a skill directory already exists, follow the prompt rather than forcing a replace.

User-only skills (`grilling`, `engineering-router`, `improve-codebase-architecture`,
`setup-agentos`) declare `disable-model-invocation: true`. Keep that field. Do not mark them
model-invocable.

This step does **not** install the CLI.

## Runtime (checksummed GitHub Release)

After a tag `vX.Y.Z` is published:

```bash
curl -fsSL https://raw.githubusercontent.com/itsmohit124/agentos/main/install/install.sh | sh
```

Safer, after reading the script:

```bash
sh install/install.sh --version v0.1.0 --prefix "$HOME/.local"
```

The script detects macOS or Linux and the CPU, downloads `agentos-<target>.tar.gz` and
`SHA256SUMS`, verifies the checksum, and writes `PREFIX/bin/agentos`. A missing artifact or
a checksum mismatch exits 1 and does not install.

Windows: `install/install.ps1`.

Until a GitHub Release exists, use a source build (Rust required):

```bash
cargo build --release
./target/release/agentos --version
```

Uninstall the binary:

```bash
sh install/install.sh --uninstall --prefix "$HOME/.local"
```

That removes only `PREFIX/bin/agentos`. It does not delete project `.agentos/` directories or
skill files. Use `agentos cleanup --yes` in a project for managed adapter blocks and `.agentos/`.

## First run in a project

```bash
agentos --version
agentos init
agentos doctor
agentos scan
```

Or invoke `/setup-agentos` in an agent that already has the skill. That skill checks for the
CLI, runs `init` if the CLI is present, and does not request API keys.

No API key is required for local scan, context, plan, evaluate, or mock runs.
