# Schemas

This directory holds machine-readable contracts introduced by the relevant phase.

Expected contracts over time:
- `skill-manifest.schema.json` — Phase 1 (**exists**). JSON Schema 2020-12 for `SKILL.md`
  front-matter. It is embedded in `agentos-skills` and every manifest is validated against it before
  semantic checks; unknown fields are rejected.
- `knowledge-node.schema.json` — Phase 2
- `knowledge-edge.schema.json` — Phase 2
- `context-pack.schema.json` — Phase 4

Do not implement a schema ahead of its phase unless the phase explicitly requires it.
