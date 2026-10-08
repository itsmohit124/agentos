# Architecture

The locked V1 architecture is specified in [spec/ARCHITECTURE.md](spec/ARCHITECTURE.md), with
product principles in [spec/VISION.md](spec/VISION.md) and per-subsystem contracts in `spec/`.

## What exists after Phase 1

```text
agentos-cli ─┬─► agentos-config ──────────────► agentos-core
             ├─► agentos-evaluation ─┬─► agentos-skills ─┬─► agentos-core
             │                       │                   └─► agentos-permissions
             ├─► agentos-skills      └─► agentos-permissions
             └─► agentos-permissions
```

| Crate | Contents |
|---|---|
| `agentos-core` | `fs` (`FileSystem` incl. sorted `read_dir` and streaming `open_read`, `RealFs`, `MemFs`), `git` (`Git`, `GitCli`, `FakeGit`), `root` (project-root detection), `state` (`.agentos/` layout, idempotent `ensure`), `db` (SQLite open, migration runner, read-only `inspect`) |
| `agentos-config` | Layered config resolution (defaults → file → env → CLI) with per-key sources |
| `agentos-permissions` | `Capability`, `PermissionSet`, `PermissionSubject`, `check(subject, capability) -> Decision` (default-deny, with reason) |
| `agentos-skills` | `SKILL.md` front-matter parsing (schema `schemas/skill-manifest.schema.json` + semantic checks), `SkillLoader` (prioritized discovery, front-matter only), `SkillRegistry` (lazy body/reference loading), dependency resolver (topological order, cycle path, semver), rule-based `SkillRouter`, full `validate`, `SkillGraph` (text/DOT), local `install`, built-in skills embedded from `skills/` |
| `agentos-evaluation` | Deterministic case runner; area `skills` (`evals/skills/*.json`) |
| `agentos-cli` | The `agentos` binary: `init`, `doctor`, `config`, `skills {list, inspect, validate, install, graph}`, `evaluate skills`, logging, exit codes |
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
