#!/usr/bin/env python3
"""Validate registry.json against nifti-registry.schema.json.

Checks JSON Schema conformance plus phantom-id uniqueness (which the schema
cannot express). Run from anywhere in the repo:

    python tools/validate_registry.py

Requires: jsonschema (`pip install jsonschema`).
"""
from __future__ import annotations

import json
import sys
from collections import Counter
from pathlib import Path

from jsonschema import Draft202012Validator

ROOT = Path(__file__).resolve().parent.parent
SCHEMA = ROOT / "nifti-registry.schema.json"
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

    # The schema cannot express "unique by key" across array items.
    ids = [p.get("id") for p in registry.get("phantoms", []) if isinstance(p, dict)]
    dupes = sorted(i for i, n in Counter(ids).items() if i is not None and n > 1)
    errors += [f"phantoms: duplicate id {dup!r}" for dup in dupes]

    if errors:
        print(f"registry.json is INVALID ({len(errors)} problem(s)):")
        for e in errors:
            print(f"  - {e}")
        return 1

    print(f"registry.json is valid: {len(ids)} phantom(s), all ids unique.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
