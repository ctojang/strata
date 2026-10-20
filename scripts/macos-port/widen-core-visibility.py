#!/usr/bin/env python3
"""Widen `pub(crate)` to `pub` in strata-core so the root crate can reach the moved items."""

from pathlib import Path

CORE = Path(__file__).resolve().parents[2] / "crates/strata-core/src"


def main():
    for path in sorted(CORE.rglob("*.rs")):
        original = path.read_text()
        updated = original.replace("pub(crate)", "pub")
        if updated != original:
            path.write_text(updated)
            print(path.relative_to(CORE.parents[2]))


if __name__ == "__main__":
    main()
