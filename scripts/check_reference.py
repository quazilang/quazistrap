#!/usr/bin/env python3
"""Check that docs/reference/prelude matches `qz doc` output for prelude/src.

Regenerate with:
    qz doc --src prelude/src --package prelude -o docs/reference/prelude
"""

import argparse
import filecmp
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--qz", type=Path, default=Path("target/debug/qz"))
    qz = parser.parse_args().qz.resolve()
    committed = ROOT / "docs/reference/prelude"
    with tempfile.TemporaryDirectory(prefix="quazi-reference-") as directory:
        generated = Path(directory)
        subprocess.run([str(qz), "doc", "--src", str(ROOT / "prelude/src"), "--package", "prelude",
                        "-o", str(generated)], check=True, capture_output=True)
        stale = []
        for page in sorted(generated.rglob("*.md")):
            relative = page.relative_to(generated)
            target = committed / relative
            if not target.exists() or not filecmp.cmp(page, target, shallow=False):
                stale.append(str(relative))
        extra = [str(p.relative_to(committed)) for p in committed.rglob("*.md")
                 if not (generated / p.relative_to(committed)).exists()]
        undocumented = sum(page.read_text().count("_Undocumented._") for page in generated.rglob("*.md"))
    if stale or extra:
        sys.exit(f"docs/reference/prelude is out of date: {stale + extra}; regenerate it (see --help)")
    if undocumented:
        sys.exit(f"{undocumented} public prelude declarations have no doc comment")
    print("PASS reference: docs/reference/prelude matches prelude/src and every public item is documented")


if __name__ == "__main__":
    main()
