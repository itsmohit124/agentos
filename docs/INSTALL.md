# Install AgentOS

`npx skills add` copies skill folders, including a setup script. It does not run that
script and does not install the CLI.

```bash
npx skills@latest add itsmohit124/agentos
setup=.agents/skills/setup-agentos/scripts/setup.sh
[ -f "$setup" ] || setup="$HOME/.agents/skills/setup-agentos/scripts/setup.sh"
sh "$setup"
```

Run that from the project directory. Project scope writes `setup.sh` into the
repository. Global scope writes it to `~/.agents/skills/setup-agentos/scripts/setup.sh`;
the script then copies AgentOS skills (front matter includes `version`) into the project. Other skills already in `~/.agents/skills` stay there. It refuses to initialize your
home directory. PromptScript can report that it does not support a global install;
the copies under `~/.agents/skills` are still installed, and `setup.sh` uses them.

## After a global install

`npx` with Global only stores the skills in `~/.agents/skills`. Cursor can read those
from any repo. The knowledge graph, the Context Pack, and the Cursor hook stay inside
each repo, so run setup once inside every repo you want AgentOS to index.

From that repo (not from your home directory):

```bash
cd /path/to/the/repo
sh ~/.agents/skills/setup-agentos/scripts/setup.sh
```

The script writes only into this repo:

| Path | What it is |
|---|---|
| `.agentos/` | Config, project profile, and the SQLite index for this repo |
| `.agents/skills/` | A copy of the skills for this repo. Existing skill folders are left as they are |
| `.cursor/hooks.json` and `.cursor/hooks/agentos-context.py` | The hook that attaches this repo's Context Pack |

The CLI binary stays at `~/.local/bin/agentos` and is installed on the first repo only.
Open a new terminal after that first run so `PATH` includes it. In every later repo,
repeat the `sh` command above. Then, inside the repo:

```bash
agentos doctor
agentos scan
agentos context build "the change you want in this repo"
```

`context build` reads this repo's `.agentos/` index. A second repo keeps its own index
and its own pack. Do not point `--root` at your home directory.

The setup script installs `agentos` into `~/.local/bin`, adds that directory to your
shell rc (`~/.zshrc`, `~/.bashrc`, or Fish config) if it is not already there, runs
`init`, installs adapters detected from files already in the project (not from the
`npx` agent picker), and runs `scan`. Cursor: `.cursor/` or `.agents/skills/`.
Claude: `.claude/` or `CLAUDE.md`. Codex: `.codex/` or `agents/openai.yaml`.
Always `generic`. Pass `--all-adapters` to install all four. It does not overwrite
existing skills or `.agentos/config.toml`. No API key is required.

`sh setup.sh` cannot change PATH in the terminal that launched it. Open a new terminal
(or `source` your rc) once so `agentos` is on PATH there.

Until a GitHub Release exists, the CLI step uses a local binary or builds from
source. If `cargo` is missing, `setup.sh` installs stable Rust with rustup first.

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
