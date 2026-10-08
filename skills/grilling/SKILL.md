---
name: grilling
description: Stress-test a plan, design or decision by asking hard questions one at a time until its assumptions and risks are explicit.
version: "0.1.0"
invocation: user
inputs:
  - name: subject
    description: The plan, design or decision to challenge.
outputs:
  - name: findings
    description: Resolved questions, accepted risks and open issues.
permissions: [fs.read]
optional_recommends: [research, domain-modeling]
triggers:
  phrases: [grill me, stress test, poke holes, play devil's advocate, challenge my]
  keywords: [grill, challenge, critique, assumptions]
  intents: [critique]
eval_cases:
  - task: Grill me about my caching strategy
  - task: Poke holes in this rollout plan before I present it
---
# Grilling

## When to use
The human explicitly asks to have a plan, design or decision challenged before committing to it.
This skill is user-invoked only: it is an intentional, interactive workflow.

## Inputs
- The subject: a plan, design document, decision or idea, plus its goal.

## Procedure
1. Summarise the subject and its goal in two sentences; ask the human to confirm.
2. List the assumptions it depends on, ordered by impact if wrong.
3. Ask one hard question at a time about the highest-impact open assumption; wait for the answer.
4. Record each answer as resolved, accepted risk, or open issue.
5. Stop when no high-impact assumption is open, or when the human ends the session.

## Outputs
A short record: resolved questions, accepted risks with owners, and open issues.

## Must not
- Ask several questions at once or answer its own questions.
- Rewrite the plan or change code.
- Run without an explicit request from the human.

## Validation
Every assumption listed in step 2 ends up resolved, accepted or open in the final record.
