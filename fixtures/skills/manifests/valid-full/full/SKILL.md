---
name: full
description: A skill using every manifest field.
version: "1.2.3-beta.1"
kind: skill
invocation: both
user_invocable: true
model_invocable: true
requires:
  - base
  - name: helper
    version: ">=0.1, <2"
optional_recommends: [extra]
inputs:
  - name: task
    description: The task.
outputs:
  - name: report
    description: The report.
    required: false
tools: [test-runner, vcs]
permissions: [fs.read, fs.write, shell, network, browser]
references: [references/notes.md]
triggers:
  phrases: [use every field]
  keywords: [full]
  intents: [testing]
  exclude: [nothing here]
eval_cases:
  - task: use every field
  - task: something else
    expect: not_ranked
license: MIT
compatibility: Any agent that reads SKILL.md.
metadata:
  owner: fixtures
allowed-tools: Read Grep
---
Body.
