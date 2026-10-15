#!/usr/bin/env python3
"""Cursor beforeSubmitPrompt hook: inject a budgeted AgentOS Context Pack.

Fails open: if AgentOS is missing or the pack cannot be built, the prompt continues
without extra context. Does not invent a token-reduction percentage; the context
engine already selects fragments to a budget.
"""
from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys


def fail_open() -> None:
    sys.stdout.write('{"continue":true}\n')
    raise SystemExit(0)


def prompt_text(data: object) -> str:
    if not isinstance(data, dict):
        return ""
    for key in ("prompt", "content", "text"):
        value = data.get(key)
        if isinstance(value, str) and value.strip():
            return value.strip()
        if isinstance(value, dict):
            inner = value.get("text")
            if isinstance(inner, str) and inner.strip():
                return inner.strip()
    return ""


def main() -> None:
    try:
        data = json.load(sys.stdin)
    except (json.JSONDecodeError, OSError):
        fail_open()

    prompt = prompt_text(data)
    if not prompt or not os.path.isdir(".agentos"):
        fail_open()

    agentos = shutil.which("agentos")
    if agentos is None:
        candidate = os.path.expanduser("~/.local/bin/agentos")
        if os.path.isfile(candidate) and os.access(candidate, os.X_OK):
            agentos = candidate
    if not agentos:
        fail_open()

    try:
        completed = subprocess.run(
            [agentos, "context", "build", prompt, "--root", "."],
            capture_output=True,
            text=True,
            timeout=25,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired):
        fail_open()

    pack = (completed.stdout or "").strip()
    if completed.returncode != 0 or not pack:
        fail_open()

    sys.stdout.write(
        json.dumps({"continue": True, "additional_context": pack}, ensure_ascii=False)
    )
    sys.stdout.write("\n")


if __name__ == "__main__":
    main()
