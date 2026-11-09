#!/usr/bin/env python3
"""Check strength reduction: integer multiply/divide/remainder by proven
powers of two lower to shifts and masks with exactly the semantics of the
general instructions (signed truncation toward zero, i64::MIN, unsigned values
above i64::MAX), natively and from source-removed QZI."""

import argparse
import subprocess
import tempfile
from pathlib import Path

PROGRAM = r"""import std.io;

fn signed_case(x: i64) void {
    const d1: i64 = 1;
    const d2: i64 = 2;
    const d8: i64 = 8;
    const dbig: i64 = 1048576;
    io.println("s {} {} {} {} {} {} {} {} {} {} {}", x, x / d1, x % d1, x / d2, x % d2, x / d8, x % d8, x / dbig, x % dbig, x * d8, d8 * x);
}

fn unsigned_case(x: u64) void {
    const d2: u64 = 2;
    const d16: u64 = 16;
    const dbig: u64 = 4294967296;
    io.println("u {} {} {} {} {} {} {}", x, x / d2, x % d2, x / d16, x % d16, x / dbig, x % dbig);
}

fn main() i32 {
    signed_case(0);
    signed_case(1);
    signed_case(-1);
    signed_case(7);
    signed_case(-7);
    signed_case(-8);
    signed_case(-9);
    signed_case(123456789);
    signed_case(-123456789);
    signed_case(9223372036854775807);
    signed_case(-9223372036854775807 - 1);
    unsigned_case(0 as u64);
    unsigned_case(17 as u64);
    unsigned_case(18446744073709551615);
    unsigned_case(9223372036854775808);
    unsigned_case(12345678901234 as u64);
    0
}
"""


def tdiv(a, b):
    q = abs(a) // abs(b)
    return q if (a >= 0) == (b > 0) else -q


def wrap(value):
    value &= (1 << 64) - 1
    return value - (1 << 64) if value >= 1 << 63 else value


def expected(line):
    kind, *values = line.split()
    x = int(values[0])
    if kind == "s":
        result = [x]
        for divisor in (1, 2, 8, 1 << 20):
            result += [tdiv(x, divisor), x - divisor * tdiv(x, divisor)]
        return result + [wrap(x * 8), wrap(8 * x)]
    result = [x]
    for divisor in (2, 16, 1 << 32):
        result += [x // divisor, x % divisor]
    return result


def verify(output):
    lines = [line for line in output.splitlines() if line[:2] in ("s ", "u ")]
    assert len(lines) == 16, output
    for line in lines:
        assert [int(value) for value in line.split()[1:]] == expected(line), line


def invoke(command, root):
    result = subprocess.run(command, cwd=root, text=True, stdout=subprocess.PIPE,
                            stderr=subprocess.STDOUT, timeout=300)
    assert result.returncode == 0, f"{command!r} returned {result.returncode}:\n{result.stdout}"
    return result.stdout


def check(qz, root):
    common = ["--no-incremental", "--no-color", "--no-unicode", "--silent"]
    case = root / "reduce"
    (case / "src").mkdir(parents=True)
    (case / "quazi.toml").write_text('[package]\nname="reduce"\n[build]\nentry="src/main.qz"\n')
    (case / "src/main.qz").write_text(PROGRAM)
    binary = case / "native"
    invoke([str(qz), "build", *common, "-o", str(binary)], case)
    verify(invoke([str(binary)], case))
    artifact = case / "program.qzi"
    invoke([str(qz), "build", *common, "-i", "-o", str(artifact)], case)
    (case / "src/main.qz").unlink()
    portable = case / "portable"
    invoke([str(qz), "build", str(artifact), *common, "-o", str(portable)], root)
    verify(invoke([str(portable)], case))
    print("PASS strength reduction: 16 signed/unsigned power-of-two cases match Python, native and source-removed QZI")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--qz", type=Path, default=Path("target/debug/qz"))
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="quazi-reduce-") as directory:
        check(args.qz.resolve(), Path(directory))


if __name__ == "__main__":
    main()
