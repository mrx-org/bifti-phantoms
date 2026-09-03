#!/usr/bin/env python3
"""Validate registry.json against bifti-registry-v1.schema.json.

Checks JSON Schema conformance. Run from anywhere in the repo:

    python tools/validate_registry.py

Requires: jsonschema (`pip install jsonschema`).
"""
from __future__ import annotations

import json
import sys
from pathlib import Path

from jsonschema import Draft202012Validator

ROOT = Path(__file__).resolve().parent.parent
SCHEMA = ROOT / "bifti-registry-v1.schema.json"
REGISTRY = ROOT / "registry.json"


def main() -> int:
    schema = json.loads(SCHEMA.read_text(encoding="utf-8"))
    registry = json.loads(REGISTRY.read_text(encoding="utf-8"))

    Draft202012Validator.check_schema(schema)
    validator = Draft202012Validator(schema)

    errors = []
    for err in sorted(validator.iter_errors(registry), key=lambda e: list(e.path)):
        loc = "/".join(str(p) for p in err.path) or "(root)"
        errors.append(f"{loc}: {err.message}")

    if errors:
        print(f"registry.json is INVALID ({len(errors)} problem(s)):")
        for e in errors:
            print(f"  - {e}")
        return 1

    # The registry is an object keyed by collection name, so names are unique by
    # construction - no extra check needed beyond the schema.
    collections = registry if isinstance(registry, dict) else {}

    n_phantoms = 0
    dup_errors = []
    for collection_name, c in collections.items():
        if not isinstance(c, dict):
            continue
        files = flatten_phantoms(c.get("phantoms", []))
        n_phantoms += len(files)
        seen = set()
        for f in files:
            if f in seen:
                dup_errors.append(f"'{collection_name}': '{f}' appears more than once")
            seen.add(f)

    if dup_errors:
        print(f"registry.json is INVALID ({len(dup_errors)} problem(s)):")
        for e in dup_errors:
            print(f"  - {e}")
        return 1

    print(
        f"registry.json is valid: {len(collections)} collection(s), "
        f"{n_phantoms} phantom(s)."
    )
    return 0


def flatten_phantoms(phantoms: list) -> list[str]:
    """Every phantom filename in a (possibly nested) phantoms list, depth-first."""
    files: list[str] = []
    for entry in phantoms:
        if isinstance(entry, str):
            files.append(entry)
        elif isinstance(entry, dict):
            files.extend(flatten_phantoms(entry.get("phantoms", [])))
    return files


if __name__ == "__main__":
    sys.exit(main())
