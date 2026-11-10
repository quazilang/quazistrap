#!/usr/bin/env python3
"""Check the documentation: relative Markdown links must resolve, and every
complete Quazi example (a ```quazi block with `fn main`) must pass `qz check`.

A block opened with ```quazi,ignore, or marked `// tutorial: fragment` or
`// tutorial: invalid` (the tutorial's fence classification, see
src/docs.rs), needs context from elsewhere or is deliberately invalid, and is
skipped.
"""

import argparse
import re
import subprocess
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
LINK = re.compile(r"\]\(([^)\s]+)\)")
BLOCK = re.compile(r"^```quazi(?P<info>[^\n]*)\n(?P<body>.*?)^```", re.S | re.M)


def markdown_files():
    yield ROOT / "README.md"
    yield from sorted((ROOT / "docs").rglob("*.md"))


def prose(text):
    """Text outside fenced blocks and inline code spans."""
    text = re.sub(r"^```.*?^```", "", text, flags=re.S | re.M)
    return re.sub(r"`[^`\n]*`", "", text)


def broken_links(path, text):
    for target in LINK.findall(prose(text)):
        if re.match(r"^[a-z]+:", target) or target.startswith("#"):
            continue
        file_part = target.split("#", 1)[0]
        if not file_part:
            continue
        if not (path.parent / file_part).exists():
            yield target


def examples(text):
    for match in BLOCK.finditer(text):
        info = match.group("info").strip()
        body = match.group("body")
        line = text.count("\n", 0, match.start()) + 1
        if ("ignore" in info or "fn main" not in body or "tutorial: fragment" in body
                or "tutorial: invalid" in body):
            continue
        yield line, body


def check_example(qz, root, index, body):
    project = root / f"example_{index}"
    (project / "src").mkdir(parents=True)
    (project / "quazi.toml").write_text(
        f'[package]\nname = "docs_example_{index}"\n\n[build]\nentry = "src/main.qz"\n')
    (project / "src/main.qz").write_text(body)
    result = subprocess.run([str(qz), "check"], cwd=project, capture_output=True, text=True,
                            timeout=120)
    return result.returncode == 0, result.stdout + result.stderr


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--qz", type=Path, default=Path("target/debug/qz"))
    args = parser.parse_args()
    qz = args.qz.resolve()
    failures = []
    links = checked = 0
    with tempfile.TemporaryDirectory(prefix="quazi-docs-") as directory:
        root = Path(directory)
        for path in markdown_files():
            text = path.read_text()
            name = path.relative_to(ROOT)
            for target in broken_links(path, text):
                failures.append(f"{name}: broken link {target}")
            links += len(LINK.findall(prose(text)))
            for line, body in examples(text):
                checked += 1
                ok, output = check_example(qz, root, checked, body)
                if not ok:
                    errors = [entry for entry in output.splitlines() if "error" in entry][:2]
                    failures.append(f"{name}:{line}: example fails qz check: {' | '.join(errors)}")
    for failure in failures:
        print(failure)
    if failures:
        raise SystemExit(f"{len(failures)} documentation problems")
    print(f"PASS docs: {links} links resolve, {checked} complete examples pass qz check")


if __name__ == "__main__":
    main()
