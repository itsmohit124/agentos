# AgentOS Vision

AgentOS is a **universal agent engineering layer**: it gives any coding agent the right knowledge,
skills, workflow, feedback and guardrails — without replacing the agent.

```text
                      AgentOS
   ┌───────────────┬───────────────┬────────────────┐
   Knowledge Engine  Skill Engine    Agent Adapters
          └──────┬────────┘            (Cursor/Claude/Codex/Generic)
                 ▼
          Context Engine → Workflow Engine → Feedback Engine → Evaluation
```

## Principles
1. Agent-agnostic  2. Language-agnostic  3. Framework-agnostic  4. Skill-driven
5. Context-efficient  6. Domain-aware  7. Evidence-based  8. Human-confirmable
9. Testable  10. Observable  11. Permission-aware  12. Incrementally adoptable

## The key rule
> The system must never give an agent more context simply because that context exists.
> Context is selected because it is relevant to the current task.

## Non-goals
- Not an LLM agent. Not a cloud service. Not a replacement for Cursor/Claude/Codex.
- No Kubernetes, Postgres, Redis, Kafka, Neo4j in core. Local-first developer tool.
