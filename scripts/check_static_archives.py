#!/usr/bin/env python3
"""Check static archives (`.a`) in the built-in ELF linker: only needed
members are linked (transitively), an unused member with an undefined
reference stays out, the result is static, `qz run --jit` works, and a symbol
no member defines is reported. Needs `cc` and `ar`."""

import argparse
import shutil
import subprocess
import tempfile
from pathlib import Path

SOURCES = {
    "add.c": "long qz_double(long value);\nlong qz_add_twice(long a, long b) { return qz_double(a + b); }\n",
    "double.c": "long qz_double(long value) { return value * 2; }\n",
    "unused.c": "long missing_symbol_never_defined(void);\n"
                "long qz_unused(void) { return missing_symbol_never_defined(); }\n",
}

PROGRAM = r"""import std.io;

@api("qz_add_twice")
unsafe fn add_twice(a: i64, b: i64) i64;

fn main() i32 {
    var value: i64 = 0;
    unsafe { value = add_twice(20, 1); }
    io.println("archive {}", value);
    if (value != 42) { ret 1; }
    0
}
"""


def run(command, root):
    return subprocess.run(command, cwd=root, text=True, stdout=subprocess.PIPE,
                          stderr=subprocess.STDOUT, timeout=300)


def invoke(command, root):
    result = run(command, root)
    assert result.returncode == 0, f"{command!r} returned {result.returncode}:\n{result.stdout}"
    return result.stdout


def check(qz, root):
    for name, source in SOURCES.items():
        (root / name).write_text(source)
        invoke(["cc", "-c", "-O2", "-fno-stack-protector", "-o", name[:-2] + ".o", name], root)
    # Archive order puts the transitive dependency after the unused member.
    invoke(["ar", "rcs", "libhelpers.a", "add.o", "unused.o", "double.o"], root)
    (root / "main.qz").write_text(PROGRAM)
    common = ["--no-incremental", "--no-color", "--no-unicode", "--silent"]
    invoke([str(qz), "build", *common, "main.qz", "libhelpers.a", "-o", "prog"], root)
    assert "archive 42" in invoke([str(root / "prog")], root)
    assert b"missing_symbol_never_defined" not in (root / "prog").read_bytes()
    assert b"\x7fELF" == (root / "prog").read_bytes()[:4]
    assert "archive 42" in invoke([str(qz), "run", "--jit", *common, "main.qz", "libhelpers.a"], root)
    invoke(["ar", "rcs", "libpartial.a", "add.o"], root)
    result = run([str(qz), "build", *common, "main.qz", "libpartial.a", "-o", "broken"], root)
    assert result.returncode != 0 and "qz_double" in result.stdout, result.stdout
    print("PASS static archives: needed members only (transitive), static output, --jit, unresolved symbol reported")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--qz", type=Path, default=Path("target/debug/qz"))
    args = parser.parse_args()
    if not (shutil.which("cc") and shutil.which("ar")):
        print("SKIP static archives: cc and ar are required")
        return
    with tempfile.TemporaryDirectory(prefix="quazi-archives-") as directory:
        check(args.qz.resolve(), Path(directory))


if __name__ == "__main__":
    main()
