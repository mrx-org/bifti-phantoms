#!/usr/bin/env python3
"""Validate the phantom JSON files in this repo against the phantom schema.

Walks the repo for files that look like a phantom (a JSON object with a
``$schema`` naming ``bifti-phantom-v1``) and checks each against
bifti-phantom-v1.schema.json. Run from anywhere in the repo:

    python tools/validate_phantoms.py

Requires: jsonschema (`pip install jsonschema`).
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path

from jsonschema import Draft202012Validator

ROOT = Path(__file__).resolve().parent.parent
SCHEMA = ROOT / "bifti-phantom-v1.schema.json"

# Same discriminator the loaders use - see JSON.md -> `$schema`.
SCHEMA_REF = re.compile(r"(nifti|bifti)-phantom-v1(\.[^/]*)?$")

SKIP_DIRS = {".git", "target", "node_modules", ".venv", "__pycache__"}


def find_phantoms() -> list[Path]:
    """Every JSON file in the repo whose `$schema` marks it as a phantom."""
    found = []
    for path in sorted(ROOT.rglob("*.json")):
        if SKIP_DIRS & set(path.relative_to(ROOT).parts):
            continue
        try:
            data = json.loads(path.read_text(encoding="utf-8"))
        except (json.JSONDecodeError, UnicodeDecodeError):
            continue
        if isinstance(data, dict) and SCHEMA_REF.search(str(data.get("$schema", ""))):
            found.append(path)
    return found


def main() -> int:
    schema = json.loads(SCHEMA.read_text(encoding="utf-8"))
    Draft202012Validator.check_schema(schema)
    validator = Draft202012Validator(schema)

    phantoms = find_phantoms()
    if not phantoms:
        print("No phantom files found - did the $schema discriminator change?")
        return 1

    failed = 0
    for path in phantoms:
        rel = path.relative_to(ROOT)
        data = json.loads(path.read_text(encoding="utf-8"))
        errors = sorted(validator.iter_errors(data), key=lambda e: list(e.path))
        if errors:
            failed += 1
            print(f"{rel} is INVALID ({len(errors)} problem(s)):")
            for err in errors:
                loc = "/".join(str(p) for p in err.path) or "(root)"
                print(f"  - {loc}: {err.message}")
        else:
            print(f"{rel} is valid.")

    if failed:
        print(f"\n{failed} of {len(phantoms)} phantom file(s) failed validation.")
        return 1

    print(f"\nAll {len(phantoms)} phantom file(s) are valid.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
