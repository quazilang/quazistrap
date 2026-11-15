#!/usr/bin/env python3
"""Check D-022 checked integer arithmetic at run time: + - * unary - / %
and ++/compound assignment panic on overflow for narrow and 64-bit, signed
and unsigned types; in-range results and wrapping_* are exact. Operands come
through functions so constant folding cannot hide the checks. Native and
QZI."""

import argparse
import subprocess
import tempfile
from pathlib import Path

HELPERS = """import std.io.println;
fn i8v(x: i8) i8 { ret x; }
fn i32v(x: i32) i32 { ret x; }
fn i64v(x: i64) i64 { ret x; }
fn u8v(x: u8) u8 { ret x; }
fn u64v(x: u64) u64 { ret x; }
"""

PANICS = {
    "i32 add": ("i32v(2147483647) + i32v(1)", "attempt to add with overflow"),
    "i8 sub": ("i8v(-128) - i8v(1)", "attempt to subtract with overflow"),
    "u8 mul": ("u8v(16) * u8v(16)", "attempt to multiply with overflow"),
    "i64 add": ("i64v(9223372036854775807) + i64v(1)", "attempt to add with overflow"),
    "i64 sub": ("i64v(-9223372036854775807) - i64v(2)", "attempt to subtract with overflow"),
    "i64 mul": ("i64v(4611686018427387904) * i64v(2)", "attempt to multiply with overflow"),
    "u64 add": ("u64v(18446744073709551615) + u64v(1)", "attempt to add with overflow"),
    "u64 sub": ("u64v(0) - u64v(1)", "attempt to subtract with overflow"),
    "u64 mul": ("u64v(4294967296) * u64v(4294967296)", "attempt to multiply with overflow"),
    "i64 neg": ("-i64v(-9223372036854775807 - 1)", "attempt to negate with overflow"),
    "u8 neg": ("-u8v(1)", "attempt to negate with overflow"),
    "i64 div": ("i64v(-9223372036854775807 - 1) / i64v(-1)", "attempt to divide with overflow"),
    "i8 rem": ("i8v(-128) % i8v(-1)", "attempt to calculate the remainder with overflow"),
}

STATEMENTS = {
    "u8 increment": ("var value: u8 = u8v(255); value++;", "attempt to add with overflow"),
    "i32 compound": ("var value: i32 = i32v(-2147483647); value -= 2;", "attempt to subtract with overflow"),
}

VALID = HELPERS + """fn main() i32 {
    println("{} {} {}", i32v(2147483646) + i32v(1), i8v(-127) - i8v(1), u8v(15) * u8v(17));
    println("{} {} {}", i64v(-4611686018427387904) * i64v(2), u64v(9223372036854775808) + u64v(9223372036854775807), -i64v(-9223372036854775807));
    println("{} {} {}", i32v(2147483647).wrapping_add(1), u64v(0).wrapping_sub(1), u8v(16).wrapping_mul(u8v(17)));
    println("{} {}", i64v(-9223372036854775807 - 1).wrapping_neg(), i64v(-9223372036854775807 - 1) / i64v(1));
    0
}
"""
EXPECTED = """2147483647 -128 255
-9223372036854775808 18446744073709551615 9223372036854775807
-2147483648 18446744073709551615 16
-9223372036854775808 -9223372036854775808
"""


def run(command, root, code=0):
    result = subprocess.run(command, cwd=root, text=True, capture_output=True, timeout=300)
    assert result.returncode == code, f"{command!r}: {result.returncode} {result.stdout}{result.stderr}"
    return result.stdout + result.stderr


def build(qz, root, source, qzi):
    (root / "src/main.qz").write_text(source)
    common = ["--no-incremental", "--no-color", "--no-unicode", "--silent"]
    binary = root / "program"
    if qzi:
        run([str(qz), "build", *common, "-i", "-o", str(root / "program.qzi")], root)
        run([str(qz), "build", str(root / "program.qzi"), *common, "-o", str(binary)], root)
    else:
        run([str(qz), "build", *common, "-o", str(binary)], root)
    return binary


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--qz", type=Path, default=Path("target/debug/qz"))
    qz = parser.parse_args().qz.resolve()
    with tempfile.TemporaryDirectory(prefix="quazi-overflow-") as directory:
        root = Path(directory)
        (root / "src").mkdir()
        (root / "quazi.toml").write_text('[package]\nname="overflow"\n[build]\nentry="src/main.qz"\n')
        cases = [(name, f"println(\"{{}}\", {expression});", message)
                 for name, (expression, message) in PANICS.items()]
        cases += [(name, statement, message) for name, (statement, message) in STATEMENTS.items()]
        for qzi in (False, True):
            binary = build(qz, root, VALID, qzi)
            assert run([str(binary)], root) == EXPECTED, run([str(binary)], root)
            for name, body, message in cases:
                source = HELPERS + "fn main() i32 {\n    " + body + "\n    0\n}\n"
                output = run([str(build(qz, root, source, qzi))], root, 101)
                assert message in output, (name, output)
    print(f"PASS integer overflow: {len(cases)} panics and in-range/wrapping results, native and QZI")


if __name__ == "__main__":
    main()
