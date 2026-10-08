# AgentOS Skill System

## Portable unit

A skill is a directory containing `SKILL.md` and optional references/scripts/assets.

```text
skill-name/
  SKILL.md
  references/
  scripts/
  assets/
```

## Compatibility

Use the Agent Skills / `SKILL.md` shape as the portable contract. Keep agent-specific files in adapters or compatibility directories.

## Invocation modes

### User-invoked

Only the human explicitly requests the skill. These skills are suitable for orchestration commands and intentional workflows.

### Model-invoked

The agent may autonomously choose the skill when the task matches its description.

A user-invoked skill must never be made model-invocable implicitly.

## Skill metadata

The AgentOS internal manifest should support:

```text
name
version
description
invocation mode
requires
optional_recommends
inputs
outputs
tools
permissions
references
evaluation_cases
```

The portable `SKILL.md` frontmatter should remain compatible with supported agents.

## Dependencies

Represent dependencies explicitly. Detect cycles.

Dependencies must resolve to a deterministic execution order or a deterministic error.

## Router skills

Provide a router concept similar to `ask-matt`: given a user goal, recommend or invoke the most appropriate user-invoked workflow.

## Progressive disclosure

Always keep descriptions concise. Load the full skill body only when the skill is selected. Load reference files only when needed.

## Skill quality rule

A skill exists to make an engineering process more predictable, not merely to store prose.

A skill should define:
- when it applies
- what it consumes
- the ordered procedure
- what it produces
- what it must not do
- how it is validated
