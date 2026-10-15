---
name: research
description: Answer a technical question from high-trust primary sources (official docs, specs, source code) and record the findings with citations.
version: "0.1.0"
invocation: both
inputs:
  - name: question
    description: The question to answer, with the decision it informs.
  - name: constraints
    description: Versions, platforms or sources that are in or out of scope.
    required: false
outputs:
  - name: findings
    description: A markdown note with the answer, cited sources and open questions.
tools: [web-search, web-fetch]
permissions: [fs.read, network]
optional_recommends: [grilling]
triggers:
  phrases: [find out, look up, gather the docs, primary sources, read the docs]
  keywords: [research, investigate, docs, documentation, sources, library, compare]
  intents: [research]
eval_cases:
  - task: Research how the HTTP client handles connection pooling
  - task: Look up the official documentation for the rate limiter
---
# Research

## When to use
A decision depends on facts you do not have yet: an API's behaviour, a library's guarantees,
a specification detail, or how something works in this codebase.

## Inputs
- The question, phrased so it has a checkable answer.
- Optional constraints: versions, platforms, sources to prefer or avoid.

## Procedure
1. Restate the question and the decision it informs in one sentence each.
2. List candidate primary sources (official docs, specs, source code, changelogs) before searching.
3. Read the sources; record each claim with its source and location.
4. Resolve contradictions by preferring the most authoritative and most recent source; note the conflict.
5. Write the findings: answer first, then evidence, then open questions.

## Outputs
A findings note: the answer, every claim with a citation, confidence, and unresolved questions.

## Must not
- Present unsourced claims as facts or invent citations.
- Change code or configuration.
- Prefer blog posts or forum answers over primary sources when primary sources exist.

## Validation
Every claim in the note has a citation that a reviewer can open, and the note answers the
original question or says explicitly why it cannot.
