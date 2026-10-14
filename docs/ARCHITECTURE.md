# Architecture

The locked V1 architecture is specified in [spec/ARCHITECTURE.md](spec/ARCHITECTURE.md), with
product principles in [spec/VISION.md](spec/VISION.md) and per-subsystem contracts in `spec/`.

## What exists after Phase 9

Phase 9 adds the four adapter crates. They install managed project files and call the existing
skill registry, context engine, workflow planner, and feedback engine. Agent-specific code stays
in `crates/agentos-adapters/`. The generic adapter is the shared contract. Cursor writes
`.cursor/rules/agentos.mdc`. Claude Code writes a marked section of `CLAUDE.md` and discovery
passthroughs under `.claude/skills/`. Codex writes a marked section of `AGENTS.md`. None of those
files executes a command. AgentOS authorization keeps a user-only skill user-only. Install writes
Cursor's `disable-model-invocation: true` and Codex's `allow_implicit_invocation: false` onto
user-only skills under `.agents/skills/`. Those controls agree with the AgentOS flag and are not
removed on uninstall. No native agent process is started. Uninstall removes the managed section
and can restore a pre-existing file byte for byte when the rest of the file was not edited. The backup used for that restore is ignored by
`.agentos/.gitignore` (`adapters/originals/`). An existing ignore file keeps its previous lines.

## What exists after Phase 8

Phase 8 adds browser validation as a capability, not a hard-wired core feature. `BrowserTool`
checks `Capability::Browser` and then calls `NullBrowser` (the default), `FakeBrowser`, or, when
the `playwright` feature is compiled, `PlaywrightBrowser`. `ValidationKind::Browser` is a Phase 5
provider. It is selected only when `expected/ui.json` exists. Screenshots and inspection files go
under `.agentos/runs/<id>/artifacts/`. The default build does not require Node.

## What exists after Phase 7

Phase 7 adds `agentos-memory`. Records live as JSON files under `.agentos/memory/` and are mirrored
in SQLite table `memory_records`. That table is separate from the knowledge schema, which stays at
version 2. Memory is a candidate source for the Phase 4 Context Engine. It is ranked, budgeted,
and traced with the graph. An empty memory store does not change a Context Pack. New records are
`proposed` until `memory confirm`. There is no canonical memory status. No model is called.

## What exists after Phase 6

Phase 6 adds `agentos-workflow`. It plans a task graph, expands skill dependencies through the
Phase 1 resolver, builds one Phase 4 Context Pack per step, and consumes Phase 5
`ValidationResult` values. Runs are files under `.agentos/runs/`. `MockRunner` is the test
executor. `ManualRunner` prints the step and records a supplied result. There is no model call
and no agent adapter.

## What exists after Phase 5

Phase 5 adds `agentos-feedback` and `agentos-tools`. Validation providers construct commands and
parse output. Every process is started by `agentos-tools` after a shell permission check, with a
timeout and a byte cap on stdout and stderr. Architecture analysis reads the existing knowledge
graph. It does not create another one. Stored results live under `.agentos/feedback/` and are
derived state.

## What exists after Phase 4

Phase 4 adds `agentos-context`. It reads the Phase 2 and Phase 3 graph and returns a Context Pack.
It does not write a second graph. `agentos benchmark baseline` measures naive file tokens before
any pack is built. `agentos context build` projects logical ids, provenance, and lifecycle status
into a pack with a retrieval trace. A cache under `.agentos/context-cache/` is derived state.

## What exists after Phase 3

Phase 3 adds a domain module in `agentos-knowledge`. It writes bounded contexts, tactical concepts,
findings, and lifecycle transitions into the same SQLite index (`domain_nodes`, `domain_edges`,
`domain_transitions`, `domain_findings`). Software nodes stay in the Phase 2 tables. `agentos domain`
confirms or rejects them. Generated markdown is under `.agentos/knowledge/domain/`.

## Crate layout

```text
agentos-cli ─┬─► agentos-config ───► agentos-core
             ├─► agentos-knowledge ─► agentos-core
             ├─► agentos-context ──► agentos-knowledge
             ├─► agentos-skills ─┬─► agentos-core
             │                   └─► agentos-permissions
             ├─► agentos-permissions
             ├─► agentos-feedback ─┬─► agentos-knowledge
             │                     ├─► agentos-permissions
             │                     └─► agentos-tools ─► agentos-permissions
             ├─► agentos-workflow ─┬─► agentos-skills
             │                     ├─► agentos-context
             │                     ├─► agentos-feedback
             │                     ├─► agentos-permissions
             │                     └─► agentos-tools
             ├─► agentos-memory ─┬─► agentos-context ──► agentos-knowledge
             │                   ├─► agentos-knowledge
             │                   └─► agentos-core
             ├─► agentos-evaluation ─┬─► agentos-core
             │                       ├─► agentos-knowledge
             │                       ├─► agentos-context
             │                       ├─► agentos-skills
             │                       ├─► agentos-permissions
             │                       ├─► agentos-feedback
             │                       ├─► agentos-tools
             │                       ├─► agentos-workflow
             │                       ├─► agentos-memory
             │                       └─► agentos-adapters/{generic,cursor,claude,codex}
             ├─► agentos-adapters/generic ─► context, skills, workflow, feedback, memory
             └─► agentos-adapters/{cursor,claude,codex} ─► agentos-adapter-generic
```

| Crate | Contents |
|---|---|
| `agentos-core` | `fs` (`FileSystem` incl. sorted `read_dir` and streaming `open_read`, `RealFs`, `MemFs`), `git` (`Git`, `GitCli`, `FakeGit`), `root` (project-root detection), `state` (`.agentos/` layout, idempotent `ensure`), `db` (SQLite open, migration runner, read-only `inspect`) |
| `agentos-config` | Layered config resolution (defaults → file → env → CLI) with per-key sources |
| `agentos-permissions` | `Capability`, `PermissionSet`, `PermissionSubject`, `check(subject, capability) -> Decision` (default-deny, with reason) |
| `agentos-skills` | `SKILL.md` front-matter parsing (schema `schemas/skill-manifest.schema.json` + semantic checks), `SkillLoader` (prioritized discovery, front-matter only), `SkillRegistry` (lazy body/reference loading), dependency resolver (topological order, cycle path, semver), rule-based `SkillRouter`, full `validate`, `SkillGraph` (text/DOT), local `install`, built-in skills embedded from `skills/` |
| `agentos-knowledge` | Automatic project profile, ignore-aware scanner, `LanguageExtractor` (Python, TypeScript, JavaScript, Go, Java, Ruby), knowledge graph in SQLite (nodes, edges, fragments, FTS5), provenance, domain tables |
| `agentos-context` | Context Pack assembly over that graph: intent, routing, lexical/graph retrieval, budget, trace, cache. Memory candidates join that pipeline. `NullSemantic` is the semantic seam |
| `agentos-memory` | Episodic, semantic, procedural, decision, and lesson records. Files under `.agentos/memory/` plus a SQLite mirror. Proposed until confirmed. Stale when a linked fingerprint changes |
| `agentos-evaluation` | Deterministic case runner; areas `skills`, `knowledge`, `ddd`, `context`, `feedback`, `architecture`, `workflow`, `memory`, and `adapters` |
| `agentos-adapter-generic` | Adapter contract, managed-file install/uninstall, generic `.agentos/adapters/generic/` instructions and `skills.json` |
| `agentos-adapter-cursor` | `.cursor/rules/agentos.mdc` instructions. Does not copy skills into `.cursor/skills/` |
| `agentos-adapter-claude` | Marked `CLAUDE.md` section and `.claude/skills/<name>/SKILL.md` passthroughs. No `allowed-tools`, no hooks |
| `agentos-adapter-codex` | Marked `AGENTS.md` section. Skills stay in `.agents/skills/` |
| `agentos-cli` | The `agentos` binary, including `adapter list|install|uninstall|doctor` and `evaluate adapters` |
| `agentos-feedback` | `ValidationProvider`, language providers, browser provider (`expected/ui.json`), stored results, architecture detectors over the existing graph |
| `agentos-tools` | Process runner (shell permission, timeout, capped stdout and stderr) and `BrowserTool` (`NullBrowser` by default, `FakeBrowser` for tests, optional `PlaywrightBrowser`) |
| `agentos-workflow` | Task graph, Phase 1 skill expansion, deterministic scheduler, resumable runs, MockRunner and ManualRunner |
| `agentos-testkit` | Dev-only: `TempProject`, `CliRunner`, fixture loader |

### Skill system data flow

```text
SkillLocation × 4 ─► SkillLoader::discover ─► Discovery { registry, diagnostics }
  (front-matter only)                              │
                 ┌─────────────┬──────────────┬────┴────────┬──────────────┐
           resolve(name)   SkillRouter     check(cap)    validate()     SkillGraph
           topo order      ranked+reasons  allow/deny    + bodies       text / dot
```

Bodies are read only by `Skill::load_body` (used by `inspect` without `--brief` and by `validate`);
reference files only by `Skill::load_reference`. Nothing in the skill system executes a skill or
calls a model.
