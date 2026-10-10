# Architecture

The locked V1 architecture is specified in [spec/ARCHITECTURE.md](spec/ARCHITECTURE.md), with
product principles in [spec/VISION.md](spec/VISION.md) and per-subsystem contracts in `spec/`.

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
             └─► agentos-evaluation ─┬─► agentos-core
                                     ├─► agentos-knowledge
                                     ├─► agentos-context
                                     ├─► agentos-skills
                                     ├─► agentos-permissions
                                     ├─► agentos-feedback
                                     ├─► agentos-tools
                                     └─► agentos-workflow
```

| Crate | Contents |
|---|---|
| `agentos-core` | `fs` (`FileSystem` incl. sorted `read_dir` and streaming `open_read`, `RealFs`, `MemFs`), `git` (`Git`, `GitCli`, `FakeGit`), `root` (project-root detection), `state` (`.agentos/` layout, idempotent `ensure`), `db` (SQLite open, migration runner, read-only `inspect`) |
| `agentos-config` | Layered config resolution (defaults → file → env → CLI) with per-key sources |
| `agentos-permissions` | `Capability`, `PermissionSet`, `PermissionSubject`, `check(subject, capability) -> Decision` (default-deny, with reason) |
| `agentos-skills` | `SKILL.md` front-matter parsing (schema `schemas/skill-manifest.schema.json` + semantic checks), `SkillLoader` (prioritized discovery, front-matter only), `SkillRegistry` (lazy body/reference loading), dependency resolver (topological order, cycle path, semver), rule-based `SkillRouter`, full `validate`, `SkillGraph` (text/DOT), local `install`, built-in skills embedded from `skills/` |
| `agentos-knowledge` | Automatic project profile, ignore-aware scanner, `LanguageExtractor` (Python, TypeScript, JavaScript, Go, Java, Ruby), knowledge graph in SQLite (nodes, edges, fragments, FTS5), provenance, domain tables |
| `agentos-context` | Context Pack assembly over that graph: intent, routing, lexical/graph retrieval, budget, trace, cache. `NullSemantic` is the semantic seam |
| `agentos-evaluation` | Deterministic case runner; areas `skills`, `knowledge`, `ddd`, `context`, `feedback`, `architecture`, and `workflow` |
| `agentos-feedback` | `ValidationProvider`, language providers, stored results, architecture detectors over the existing graph |
| `agentos-tools` | Process runner: shell permission, timeout, capped stdout and stderr |
| `agentos-workflow` | Task graph, Phase 1 skill expansion, deterministic scheduler, resumable runs, MockRunner and ManualRunner |
| `agentos-cli` | The `agentos` binary: `init`, `doctor`, `config`, `project profile`, `scan`, `knowledge`, `domain`, `benchmark baseline`, `context`, `skills`, `validate`, `architecture analyze`, `plan`, `run`, `evaluate {skills, knowledge, ddd, context, feedback, architecture, workflow}` |
| `agentos-testkit` | Dev-only: `TempProject`, `CliRunner`, fixture loader |
| all other crates | Compiling stubs, implemented by later phases |

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
