---
name: setup-agentos
description: First-run setup for AgentOS. Check whether the CLI is installed, initialize the current project, and explain permissions without overwriting user files.
version: "0.1.0"
invocation: user
disable-model-invocation: true
permissions: [fs.read]
triggers:
  phrases: [set up agentos, install agentos, initialize agentos, first run]
  keywords: [agentos, setup, install, init]
  intents: [setup]
eval_cases:
  - task: Set up AgentOS in this repository
  - task: Write tests first for the date parser
    expect: not_ranked
---
# Set up AgentOS

This skill ships the setup script that `npx skills add` copies into the project.
`npx` only copies files. It does not run the script and does not install the CLI.

After skills are copied, the human runs the setup script from the project directory.
Project scope writes it at `.agents/skills/setup-agentos/scripts/setup.sh`.
Global scope writes it at `~/.agents/skills/setup-agentos/scripts/setup.sh`.

```bash
setup=.agents/skills/setup-agentos/scripts/setup.sh
[ -f "$setup" ] || setup="$HOME/.agents/skills/setup-agentos/scripts/setup.sh"
sh "$setup"
```

That installs the `agentos` binary (release if published, otherwise a source build;
Rust is installed with rustup when `cargo` is missing),
copies globally installed skills into the project when the project does not have them yet,
runs `init`, installs adapters that match files already in the project (Cursor when
`.cursor/` or `.agents/skills/` exists; Claude when `.claude/` or `CLAUDE.md` exists;
Codex when `.codex/` or `agents/openai.yaml` exists; always `generic`), and runs
`scan`. Use `--all-adapters` to install every adapter. It does not overwrite existing
skills or `.agentos/config.toml`. It refuses to initialize the home directory.
A global `npx` install can report that PromptScript does not support global skills.
The copies under `~/.agents/skills` are still installed, and this script uses those files.

After a global install, the human repeats this from every repo they want indexed,
not from their home directory:

```bash
cd /path/to/the/repo
sh ~/.agents/skills/setup-agentos/scripts/setup.sh
```

That writes `.agentos/` (config, profile, SQLite index), `.agents/skills/`, and the
Cursor context hook into that repo. Another repo keeps a separate `.agentos/`. The
CLI in `~/.local/bin` is installed on the first run only.

## When to use
The human asked to install, initialize, or first-run AgentOS, or the project has skills from
this repository and no `.agentos/` yet.

## Inputs
- The current project directory.
- Whether `agentos` is already on PATH.
- The bundled script at `.agents/skills/setup-agentos/scripts/setup.sh`, or
  `~/.agents/skills/setup-agentos/scripts/setup.sh` after a global install.

## Procedure
1. Detect coding agents from **files**, not binaries. Look for `.cursor/`, `CLAUDE.md`,
   `AGENTS.md`, `.codex/`, and `.claude/`. A missing `cursor`, `claude`, or `codex` binary is
   not a failure. Record what you found.
2. Check the CLI: run `agentos --version`. If that fails, tell the human to run the
   setup script from this project and stop. Use
   `.agents/skills/setup-agentos/scripts/setup.sh` when that file exists, otherwise
   `~/.agents/skills/setup-agentos/scripts/setup.sh`. Do not invent another download.
   The script verifies release checksums. Do not skip that. Do not point `--root` at
   the home directory.
3. If the CLI is present, run `agentos doctor --root .`. An uninitialized project is expected
   to fail some checks. Then run `agentos init --root .`. Init is idempotent and does not
   overwrite `.agentos/config.toml`.
4. Explain the next commands, without running anything that needs `shell`, `network`, or
   `browser` unless the human grants them: `agentos scan`, `agentos context build "<task>"`,
   `agentos plan "<task>"`, `agentos evaluate`.
5. Permissions are default-deny. `plan` may list `network` because the shipped `research`
   skill declares it. Do not widen that list. Do not ask for API keys for local workflows.
   AgentOS does not send telemetry unless `telemetry.enabled` is set, and this build still
   does not transmit.
6. Leave `.agents/skills/`, `agents/openai.yaml`, and any text outside managed adapter
   markers unchanged. Do not restore pre-seal bytes. Do not copy skills into a second tree.

## Outputs
- Whether `agentos` is on PATH and its version, or the setup script that is still needed
- Which agent config directories exist
- Whether `.agentos/` was created
- The permission list the human would grant for a later `plan` or `validate`

## Must not
- Claim that `npx skills add` installed the Rust CLI or ran `init`.
- Overwrite `.agentos/config.toml` or user skill files.
- Start Cursor, Claude Code, or Codex, or treat a missing binary as a pass.
- Request API keys for local scan, context, plan, or evaluate.

## Validation
The project has `.agentos/` only after a successful `agentos init`, user files are unchanged,
and the human was told to run the bundled `setup.sh` if the CLI is missing.
