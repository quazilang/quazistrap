#!/usr/bin/env python3
"""Run examples that the linker matrix does not cover, in a scratch copy:

- 20-c-variadic-functions: C `printf` output reaches a pipe (libc stdio is
  flushed when the program ends);
- 30-dynamic-libraries: loads libc at run time and unloads it once;
- 32-testing: `qz test` passes, as its README promises."""

import argparse
import shutil
import subprocess
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def copy(name, root):
    target = root / name
    shutil.copytree(ROOT / "examples" / name, target,
                    ignore=shutil.ignore_patterns("build", ".quazi"))
    return target


def run(command, cwd):
    return subprocess.run(command, cwd=cwd, text=True, capture_output=True, timeout=600)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--qz", type=Path, default=Path("target/debug/qz"))
    qz = str(parser.parse_args().qz.resolve())
    with tempfile.TemporaryDirectory(prefix="quazi-examples-") as directory:
        root = Path(directory)
        variadic = copy("20-c-variadic-functions", root)
        result = run([qz, "run", "--no-color", "--silent", "--no-incremental"], variadic)
        assert result.returncode == 0 and "40 + 2 = 42" in result.stdout, (
            result.returncode, result.stdout, result.stderr)
        dynamic = copy("30-dynamic-libraries", root)
        result = run([qz, "run", "--no-color", "--silent", "--no-incremental"], dynamic)
        assert result.returncode == 0 and "loaded libc.so.6 dynamically" in result.stdout, (
            result.returncode, result.stdout, result.stderr[-500:])
        testing = copy("32-testing", root)
        result = run([qz, "test", "--no-color"], testing)
        assert result.returncode == 0 and "0 failed" in result.stdout + result.stderr, (
            result.returncode, result.stdout[-1500:], result.stderr[-1500:])
    print("PASS examples: 20 prints through a pipe, 30 loads and unloads libc, 32 passes qz test")


if __name__ == "__main__":
    main()
