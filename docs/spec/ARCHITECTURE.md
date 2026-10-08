# AgentOS Architecture (locked for V1)

## Product boundary

AgentOS is a local-first, agent-agnostic engineering intelligence and workflow layer. It augments coding agents such as Cursor, Claude Code, and Codex. It is not itself the primary coding-agent UI.

## Technology
| Component | Choice |
|---|---|
| Core + CLI | Rust (Cargo workspace), single native binary |
| Knowledge store | SQLite |
| Search | SQLite FTS5 + structural/graph; optional embeddings later behind a trait |
| Code parsing | Tree-sitter (introduced Phase 2) |
| Config | TOML for project config; YAML front-matter where a portable skill format requires it |
| Skills | Portable `SKILL.md` packages; project-local `.agents/skills/` |
| Project state | `.agentos/` |
| Git | Git CLI behind an abstraction; libgit2 optional |
| Testing | Rust tests + fixture projects + golden tests |
| Evaluation | Custom deterministic evaluation harness |
| Browser | Capability abstraction; Playwright adapter in Phase 8 |
| Agent integration | Adapter architecture in Phase 9 |

## Core principles

1. Source artifacts remain the source of truth.
2. The knowledge graph is derived/indexed state.
3. Core interfaces are technology-agnostic and agent-agnostic.
4. Context is retrieved progressively and budgeted explicitly.
5. Skills are portable `SKILL.md` packages.
6. Permissions are explicit and default-deny.
7. Inferred domain knowledge never silently becomes canonical.
8. Every important assertion and relationship has provenance.
9. Deterministic behavior is preferred for discovery, routing, retrieval, and evaluation.
10. New infrastructure requires measured justification.

## High-level architecture

```text
CLI / Agent entrypoint
        |
        v
+-----------------------+
| AgentOS Core          |
+-----------+-----------+
            |
   +--------+---------+----------------+
   |        |         |                |
   v        v         v                v
Project   Knowledge Context   Skills   Workflow
Understand  Engine    Engine   Engine  Engine
   |        |         |                |
   +--------+---------+----------------+
            |
            v
      Feedback Engine
            |
            v
   Tools / Capabilities
            |
            v
   Agent-specific adapters
```

## Storage boundary

```text
Git/filesystem = source of truth
SQLite          = derived index/state/evaluation records
```

Human-readable artifacts such as ADRs, ubiquitous-language docs, context maps, and phase reports remain plain files.

## Project understanding

Project understanding runs before or alongside knowledge extraction and is automatic; the user does not choose “greenfield” vs “existing” as a setup step. It produces a deterministic, evidence-backed project profile that downstream phases consume.

The profile has two dimensions:

- `classification`: `GREENFIELD`, `EXISTING`, or `HYBRID`
- `maturity`: `EMPTY`, `SPECIFIED`, `SKELETON`, `DEVELOPING`, `ESTABLISHED`, or `MATURE`

The detector records the signals used (Git history, source files, build/package manifests, tests, docs/specs, schema/API definitions, CI/container config) plus confidence. Classification is descriptive; the maturity/profile must never invent business facts. Ambiguous cases remain explicitly uncertain rather than forcing a false classification.

Greenfield, existing, and hybrid projects converge into the same Knowledge Model: existing projects are primarily reverse-engineered from evidence, while greenfield projects can contribute declared/proposed knowledge from requirements and design artifacts.

## Core knowledge model

```text
KnowledgeNode { id, node_type, name, scope, summary, status, provenance,
                confidence, version, fingerprint, source_refs,
                retrieval_metadata, payload }

KnowledgeEdge { id, from, relation_type, to, confidence, provenance, metadata }

KnowledgeFragment { id, node_id, source_ref, location, content,
                    fingerprint, token_estimate, kind }
```

Nodes describe what exists. Edges describe relationships. Fragments are the smallest retrievable evidence units.

## Dependency direction

```text
CLI -> subsystem interfaces -> subsystem implementations
Knowledge <- shared domain model
Context -> Knowledge interfaces
Skills -> skill model + capability interfaces
Workflow -> Skills + Context + Feedback
Adapters -> stable AgentOS interfaces
```

Subsystems must not depend on agent-specific adapters. Language-specific behavior lives behind extractor/provider interfaces.

## Deep modules

Each crate should expose the smallest stable interface required by consumers and hide parsing, indexing, ranking, persistence, and execution complexity internally.

## Project layout

```text
agentos/
├── README.md  CONTRIBUTING.md  Cargo.toml
├── crates/
│   ├── agentos-cli  agentos-core  agentos-config  agentos-knowledge
│   ├── agentos-context  agentos-skills  agentos-workflow  agentos-memory
│   ├── agentos-feedback  agentos-evaluation  agentos-permissions  agentos-tools
│   └── agentos-adapters/{generic,cursor,claude,codex}
├── skills/
├── schemas/
├── fixtures/
├── evals/
├── docs/
└── scripts/
```

## Technology adapters
Language/test-runner/build knowledge lives behind capability interfaces: *what language is this? how do I parse it? run tests? type-check? lint?* The core depends on these interfaces, never on concrete commands.

## Project state layout
```text
.agentos/
├── config.toml
├── agentos.db            # SQLite, derived index (rebuildable)
├── project-profile.json  # Phase 2 (derived)
├── skills/               # project-managed skills (optional)
├── knowledge/domain/     # generated markdown (Phase 3)
├── memory/               # Phase 7
└── runs/                 # Phase 6+ run state and artifacts
```
Portable skills live in `.agents/skills/`.
