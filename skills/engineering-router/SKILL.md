---
name: engineering-router
description: Recommend the engineering skill that fits a goal (research, grilling, modelling, design, TDD, implementation, review, architecture) using each skill's declared triggers.
version: "0.1.0"
kind: router
invocation: user
routes:
  - research
  - grilling
  - domain-modeling
  - interface-design
  - tdd
  - implementation
  - code-review
  - architecture-review
  - improve-codebase-architecture
inputs:
  - name: goal
    description: What the human wants to achieve, in their own words.
outputs:
  - name: recommendation
    description: Ranked skills with the matched triggers that explain each rank.
triggers:
  phrases: [which skill, what should i do next]
eval_cases:
  - task: Grill me on this API design
    skill: grilling
  - task: Write tests first for the date parser
    skill: tdd
  - task: Review the code in this pull request
    skill: code-review
---
# Engineering router

## When to use
The human has a goal but has not chosen a workflow. The router recommends one of its routes;
the ranking itself is computed deterministically by AgentOS from each route's declared triggers.

## Inputs
- The goal, in the human's words.

## Procedure
1. Pass the goal to the AgentOS router restricted to this skill's routes.
2. Present the top-ranked skill with the triggers that matched.
3. Present up to two alternatives when their scores are close.
4. If nothing matched, say so and ask the human to rephrase or pick a skill.

## Outputs
A recommendation: ranked skills, each with its matched triggers.

## Must not
- Execute the recommended skill without the human's confirmation.
- Recommend skills outside its routes.
- Rank by anything other than the deterministic router output.

## Validation
The recommendation equals the router output for the same goal (same input, same ranking).
