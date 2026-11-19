#!/usr/bin/env python3
"""Check f64 formatting of values beyond the i64 range, of tiny values (a
negative exponent instead of 0) and exponent/hex literals,
and that `parse_f64` returns the nearest f64 (randomized against Python)."""

import argparse
import math
import random
import struct
import subprocess
import tempfile
from pathlib import Path

PROGRAM = """import std.io;
fn main() i32 {
    io.println("{} {} {} {}", 1.0e300, -2.5e20, 1.0e18, 123456789012.5);
    io.println("{:.2} {} {} {}", 6.022e23, 1.7976931348623157e308, 0.5, 0xFF);
    io.println("{} {} {} {} {}", 2.5e-7, 1e-300, -3.25e-9, 0.000123, 5e-324);
    io.println("{:.3} {}", 2.5e-7, 0.0);
    0
}
"""
EXPECTED = ("1e+300 -2.5e+20 1e+18 123456789012.5\n6.02e+23 1.797693e+308 0.5 255\n"
            "2.5e-7 1e-300 -3.25e-9 0.000123 4.940656e-324\n0.000 0\n")


def parse_program():
    """Every check compares `parse_f64(repr(x))` with the literal `x`."""
    random.seed(11)
    values = [0.3, -0.3, 0.1, 1e23, 5e-324, 2.2250738585072014e-308, 1.7976931348623157e308,
              9007199254740993.0, 123.456789012345678, 3.14159265358979323846]
    while len(values) < 400:
        x = struct.unpack("<d", struct.pack("<Q", random.getrandbits(64)))[0]
        if math.isfinite(x):
            values.append(x)
    lines = ["import std.io;", "fn check(text: str, expected: f64) i32 {",
             "    const owned = String.from(text);",
             "    if (owned.parse_f64().unwrap() != expected) { io.println(\"wrong {}\", text); ret 1; }",
             "    ret 0;", "}", "fn main() i32 {", "    var wrong: i32 = 0;"]
    for x in values:
        literal = repr(x)
        mantissa, _, exponent = literal.partition("e")
        if "." not in mantissa:
            mantissa += ".0"
        literal = mantissa + ("e" + exponent if exponent else "")
        lines.append(f'    wrong += check("{repr(x)}", {literal});')
    lines += ['    io.println("wrong {}", wrong);', "    ret 0;", "}"]
    return "\n".join(lines) + "\n"


def run(command, root):
    result = subprocess.run(command, cwd=root, text=True, capture_output=True, timeout=120)
    assert result.returncode == 0, f"{command!r}: {result.stdout}{result.stderr}"
    return result.stdout


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--qz", type=Path, default=Path("target/debug/qz"))
    args = parser.parse_args()
    qz = args.qz.resolve()
    common = ["--no-incremental", "--no-color", "--no-unicode", "--silent"]
    with tempfile.TemporaryDirectory(prefix="quazi-float-") as directory:
        root = Path(directory)
        (root / "src").mkdir()
        (root / "quazi.toml").write_text('[package]\nname="float_format"\n[build]\nentry="src/main.qz"\n')
        (root / "src/main.qz").write_text(PROGRAM)
        run([str(qz), "build", *common, "-o", str(root / "native")], root)
        assert run([str(root / "native")], root) == EXPECTED
        run([str(qz), "build", *common, "-i", "-o", str(root / "program.qzi")], root)
        run([str(qz), "build", str(root / "program.qzi"), *common, "-o", str(root / "portable")], root)
        assert run([str(root / "portable")], root) == EXPECTED
        parse = root / "parse"
        (parse / "src").mkdir(parents=True)
        (parse / "quazi.toml").write_text('[package]\nname="float_parse"\n[build]\nentry="src/main.qz"\n')
        (parse / "src/main.qz").write_text(parse_program())
        run([str(qz), "build", *common, "-o", str(parse / "native")], parse)
        assert run([str(parse / "native")], parse) == "wrong 0\n"
    print("PASS float format: parse_f64 is nearest for 400 values; values >= 1e18 and nonzero values < 1e-4 use scientific notation; hex/exponent literals; native and QZI")


if __name__ == "__main__":
    main()
