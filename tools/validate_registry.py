#!/usr/bin/env python3
"""Validate registry.json against nifti-registry.schema.json.

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

    if errors:
        print(f"registry.json is INVALID ({len(errors)} problem(s)):")
        for e in errors:
            print(f"  - {e}")
        return 1

    # The registry is an object keyed by collection name, so names are unique by
    # construction - no extra check needed beyond the schema.
    collections = registry if isinstance(registry, dict) else {}
    n_phantoms = sum(
        len(c.get("phantoms", [])) for c in collections.values() if isinstance(c, dict)
    )
    print(
        f"registry.json is valid: {len(collections)} collection(s), "
        f"{n_phantoms} phantom(s)."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
