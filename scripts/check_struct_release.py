#!/usr/bin/env python3
"""Check that struct storage shells are released: a loop creating 20000
scalar-only structs (one moved into a function, one dropped locally) keeps
the process's mapped pages flat (Linux), and nested structs release both
shells."""

import argparse
import re
import subprocess
import tempfile
from pathlib import Path

PROGRAM = r"""import std.io;
import std.fs;

pub struct Pair { a: i64, b: i64, }

fn sum(p: Pair) i64 { p.a + p.b }

fn mapped_pages() u64 {
    const result = fs.read_to_string("/proc/self/statm");
    if (result.is_err()) { ret 0; }
    const text: String = result.unwrap();
    const end: usize = text.as_str().find(" ").unwrap();
    const field = text[0:end];
    const parsed: Result[u64, ParseError] = field.parse[u64]();
    parsed.unwrap_or(0)
}

fn main() i32 {
    var total: i64 = 0;
    var before: u64 = 0;
    for i : 0..20000 {
        if (i == 1000) { before = mapped_pages(); }
        const p = Pair { a: 1, b: 2 };
        total = total + sum(p);
        const q = Pair { a: 3, b: 4 };
        total = total + q.a;
    }
    io.println("total {} before {} after {}", total, before, mapped_pages());
    0
}
"""


def invoke(command, root):
    result = subprocess.run(command, cwd=root, text=True, stdout=subprocess.PIPE,
                            stderr=subprocess.STDOUT, timeout=300)
    assert result.returncode == 0, f"{command!r} returned {result.returncode}:\n{result.stdout}"
    return result.stdout


def check(qz, root):
    case = root / "shells"
    (case / "src").mkdir(parents=True)
    (case / "quazi.toml").write_text('[package]\nname="shells"\n[build]\nentry="src/main.qz"\n')
    (case / "src/main.qz").write_text(PROGRAM)
    output = invoke([str(qz), "run", "--no-incremental", "--no-color", "--silent"], case)
    match = re.search(r"total (\d+) before (\d+) after (\d+)", output)
    assert match, output
    total, before, after = (int(value) for value in match.groups())
    assert total == 120000, output
    # 38000 leaked structs would add about 38000 pages.
    assert after - before < 1000, output
    print(f"PASS struct shells: 40000 structs, mapped pages {before} -> {after}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--qz", type=Path, default=Path("target/debug/qz"))
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="quazi-shells-") as directory:
        check(args.qz.resolve(), Path(directory))


if __name__ == "__main__":
    main()
