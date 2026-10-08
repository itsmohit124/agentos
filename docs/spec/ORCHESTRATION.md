# Workflow and Orchestration Contract

AgentOS workflows are explicit graphs of steps, skills, capabilities, inputs, outputs, and dependencies.

## Principles

- user intent remains authoritative
- workflow steps are deterministic when possible
- independent steps may execute in parallel
- dependent steps wait for their prerequisites
- permissions are checked before execution
- every step has observable state
- runs can resume without repeating completed work
- failed validation may enter a bounded fix loop

## Task graph

A workflow node may consume/produce knowledge, context packs, files, tests, or structured results. Dependencies form a DAG; cycles must fail before execution.

## Execution state

Runs should distinguish at least:
`planned`, `ready`, `running`, `passed`, `failed`, `blocked`, `aborted`, `completed`.

## Context

Each step receives the smallest Context Pack justified by its task and dependencies. Full repository context is never the default.
