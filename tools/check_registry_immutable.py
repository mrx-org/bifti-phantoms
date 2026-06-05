#!/usr/bin/env python3
"""Check that no existing registry.json entry was modified or removed.

Compares the working-tree registry.json against origin/main. On main itself
there is nothing to compare, so the script exits 0 silently.

Run locally:
    python tools/check_registry_immutable.py

The same script runs in CI on every pull_request.
"""
from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
REGISTRY = ROOT / "registry.json"


def main() -> int:
    result = subprocess.run(
        ["git", "show", "origin/main:registry.json"],
        capture_output=True,
        text=True,
        cwd=ROOT,
    )
    if result.returncode != 0:
        print("Could not read registry.json from origin/main — skipping immutability check.")
        return 0

    main_registry: dict = json.loads(result.stdout)
    pr_registry: dict = json.loads(REGISTRY.read_text(encoding="utf-8"))

    errors = []
    for key, value in main_registry.items():
        if key not in pr_registry:
            errors.append(f"  '{key}': removed — existing entries cannot be deleted")
        elif pr_registry[key] != value:
            errors.append(f"  '{key}': modified — existing entries are frozen; add a new collection instead")

    if errors:
        print(f"Registry immutability check FAILED ({len(errors)} violation(s)):")
        for e in errors:
            print(e)
        return 1

    added = [k for k in pr_registry if k not in main_registry]
    suffix = f" New: {', '.join(added)}." if added else ""
    print(f"Immutability check passed.{suffix}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
