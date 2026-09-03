#!/usr/bin/env python3
"""Validate catalog.json against bifti-catalog.schema.json.

Checks JSON Schema conformance, then verifies every catalog entry resolves to a
current collection in registry.json. Run from anywhere in the repo:

    python tools/validate_catalog.py

Requires: jsonschema (`pip install jsonschema`).
"""
from __future__ import annotations

import json
import sys
from pathlib import Path

from jsonschema import Draft202012Validator

ROOT = Path(__file__).resolve().parent.parent
SCHEMA = ROOT / "bifti-catalog.schema.json"
CATALOG = ROOT / "catalog.json"
REGISTRY = ROOT / "registry.json"


def main() -> int:
    schema = json.loads(SCHEMA.read_text(encoding="utf-8"))
    catalog = json.loads(CATALOG.read_text(encoding="utf-8"))
    registry = json.loads(REGISTRY.read_text(encoding="utf-8"))

    Draft202012Validator.check_schema(schema)
    validator = Draft202012Validator(schema)

    errors = []
    for err in sorted(validator.iter_errors(catalog), key=lambda e: list(e.path)):
        loc = "/".join(str(p) for p in err.path) or "(root)"
        errors.append(f"{loc}: {err.message}")

    if errors:
        print(f"catalog.json is INVALID ({len(errors)} problem(s)):")
        for e in errors:
            print(f"  - {e}")
        return 1

    # Every catalog value must name a current registry collection.
    known = registry if isinstance(registry, dict) else {}
    entries = catalog if isinstance(catalog, dict) else {}
    unresolved = [
        f"'{label}' -> '{name}' is not a collection in registry.json"
        for label, name in entries.items()
        if name not in known
    ]

    if unresolved:
        print(f"catalog.json is INVALID ({len(unresolved)} problem(s)):")
        for e in unresolved:
            print(f"  - {e}")
        return 1

    print(f"catalog.json is valid: {len(entries)} entry(s), all resolve.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
