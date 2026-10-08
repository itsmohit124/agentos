---
name: bad-req
description: Dependency constraint is not a semver requirement.
version: "0.1.0"
requires:
  - name: base
    version: "about two"
eval_cases:
  - task: x
---
Body.
