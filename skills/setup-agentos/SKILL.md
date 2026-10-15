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

This skill configures AgentOS in the current project. It does not compile Rust, does not
download a binary, and does not grant extra permissions. `npx skills add` copies skill files
only. The CLI is a separate install.

## When to use
The human asked to install, initialize, or first-run AgentOS, or the project has skills from
this repository and no `.agentos/` yet.

## Inputs
- The current project directory.
- Whether `agentos` is already on PATH.

## Procedure
1. Detect coding agents from **files**, not binaries. Look for `.cursor/`, `CLAUDE.md`,
   `AGENTS.md`, `.codex/`, and `.claude/`. A missing `cursor`, `claude`, or `codex` binary is
   not a failure. Record what you found.
2. Check the CLI: run `agentos --version`. If that fails, print the documented install
   commands from `docs/INSTALL.md` and stop for the human. Do not pipe a download into a
   shell unless the human asked, and never skip checksum verification.
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
- Whether `agentos` is on PATH and its version, or the install command that is still needed
- Which agent config directories exist
- Whether `.agentos/` was created
- The permission list the human would grant for a later `plan` or `validate`

## Must not
- Claim that `npx skills add` installed the Rust CLI.
- Overwrite `.agentos/config.toml` or user skill files.
- Start Cursor, Claude Code, or Codex, or treat a missing binary as a pass.
- Request API keys for local scan, context, plan, or evaluate.

## Validation
The project has `.agentos/` only after a successful `agentos init`, user files are unchanged,
and the human was told whether the CLI is still missing.
