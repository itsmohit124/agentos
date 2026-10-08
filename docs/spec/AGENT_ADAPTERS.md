# Agent Adapters

## Principle

AgentOS core concepts remain unchanged regardless of which coding agent is used.

```text
AgentOS Core
   |
   +-- Cursor adapter
   +-- Claude Code adapter
   +-- Codex adapter
   +-- Generic adapter
```

## Adapter responsibilities

An adapter may manage:
- skill installation/discovery paths
- agent-specific metadata
- invocation syntax
- capability exposure
- context handoff
- lifecycle hooks

An adapter must not redefine domain or knowledge semantics.

## Compatibility direction

Prefer shared portable formats first. Generate compatibility artifacts only where an agent requires them.

## Current ecosystem assumption

Cursor, Codex, and Claude Code all support the general `SKILL.md`-based skills pattern, while their invocation and metadata conventions differ. AgentOS should preserve the portable core and isolate those differences in adapter code.

## Adapter tests

Every adapter should eventually prove:

1. install/discovery works
2. skills are visible
3. user-invoked/model-invoked semantics remain correct
4. context handoff is possible
5. no core knowledge model changes are required
