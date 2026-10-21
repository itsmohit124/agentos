# AgentOS

AgentOS is a local layer that sits next to Cursor, Claude Code, Codex, or another coding agent. It does not write the code and it does not call a model. It prepares the task: the relevant project knowledge, the skills to follow, and the checks afterward. Nothing is sent off the machine.

## What it does

`agentos scan` reads the repo into a local knowledge graph (code structure plus domain concepts such as payments or users). Each fact keeps its source. For a task, `agentos context build` returns one Context Pack: the nodes and file fragments that match that task, and a trace of why each item was kept or left out. The same task produces the same pack.

Skills are portable `SKILL.md` packages with explicit permissions (read, write, shell, network, browser), denied unless granted. Inferred domain knowledge stays proposed until a person confirms it.

## With the agents you already use

Install once per project.

- Cursor gets a hook that attaches a budgeted pack before a prompt is submitted. If the CLI is missing, the prompt still goes through.
- Claude Code gets a managed section in `CLAUDE.md`.
- Codex gets one in `AGENTS.md`.
- Any other agent can use the generic adapter, or you can run `agentos context build` and hand it the pack.

The graph, skills, and confirmed knowledge stay in the repo, so switching agents does not mean re-teaching the project. AgentOS can also plan a workflow, run the project's tests, and record a lesson from a failed run for a person to confirm later.

## How it compares

These tools all help an agent see a codebase. They hand that knowledge over in different ways.

| Tool | What it is for | How the agent gets it |
|---|---|---|
| **AgentOS** | One task-sized Context Pack, plus skills, permissions, a workflow, and domain facts a person confirms | Local. Cursor hook, a section in `CLAUDE.md` or `AGENTS.md`, or `agentos context build` |
| **[Graphify](https://graphify.com/)** | A queryable code graph: who calls what, the path between two symbols, an explanation of one node | A skill or MCP. Code is parsed locally; a cloud graph is optional. Docs and media can use a model |
| **[Aider repo map](https://aider.chat/docs/repomap.html)** | A ranked outline of the important classes and functions, trimmed to a token budget | Built into Aider and sent with each Aider request |
| **[Repomix](https://repomix.com/)** | The repo packed into one file an LLM can read. A compress mode keeps signatures and drops bodies | Paste the file, or use its MCP server. It does not compute call edges |
| **[Greptile](https://www.greptile.com/docs/how-greptile-works/graph-based-codebase-context)** | A hosted codebase graph and a written knowledge base, used to review a pull request | Comments on the PR. Cursor, Claude Code, and Codex can also connect over MCP |
| **[Serena](https://oraios.github.io/serena/)** | Live symbol tools: find a symbol, find references, rename, read diagnostics | An MCP server backed by a language server. The agent calls the tools during the session |

Graphify and Serena are for questions the agent asks while it works: callers, paths, renames. Repomix is for handing a model the whole tree at once. Aider's map stays inside Aider. Greptile reviews the pull request from a graph it hosts.

AgentOS is the shared layer in front of whichever agent the person is using. It decides what that one task should see, records why, and keeps skills and permissions the same in Cursor, Claude Code, and Codex. Its code extractors today are Python, TypeScript, JavaScript, Go, Java, and Ruby, a narrower set than Graphify or Serena.

## Try it

`npx skills add` only copies the skill files, including the setup script. It does not install the CLI and it does not index the repo. When it asks for a scope, pick one of the two below. Until a GitHub Release exists, setup uses an `agentos` binary already on `PATH` or builds it from source. Open a new terminal afterward so `~/.local/bin` is on `PATH`.

### Local install

Run this from the project you want AgentOS to manage, and choose the project scope. Skills land in that repo at `.agents/skills/`.

```bash
cd /path/to/the/repo
npx skills@latest add itsmohit124/agentos
sh .agents/skills/setup-agentos/scripts/setup.sh
agentos doctor
agentos context build "describe the change you actually want"
```

Setup writes this repo's index to `.agentos/`, installs the CLI into `~/.local/bin` if it is missing, and attaches the Context Pack for the agents already present in the repo.

### Global install

Choose the global scope. Skills land in `~/.agents/skills/`. PromptScript can report that it does not support a global install; the copies under `~/.agents/skills` are still installed.

```bash
npx skills@latest add itsmohit124/agentos
```

The graph and the hook stay inside each repo. From every repo you want indexed, run the setup script that the global install stored. Run it from that repo. The script refuses your home directory.

```bash
cd /path/to/the/repo
sh ~/.agents/skills/setup-agentos/scripts/setup.sh
agentos doctor
agentos context build "describe the change you actually want"
```

That stores this repo's index in `.agentos/`, copies the skills into `.agents/skills/`, and attaches its Context Pack from there. The CLI is installed on the first run only. The next repo gets its own `.agentos/` from the same `sh` command.
