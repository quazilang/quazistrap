#!/usr/bin/env python3
"""Check numeric `as` casts at run time: integer narrowing truncates and
re-extends, integer to float rounds to nearest, float to integer truncates and
saturates (NaN is 0), and f64 to f32 rounds. Runtime operands come through
functions so constant folding cannot hide the lowering; constant operands
check the compile-time folding gives the same answers. `as` binds tighter
than unary minus, so negative operands are parenthesized. Native and QZI."""

import argparse
import subprocess
import tempfile
from pathlib import Path

PROGRAM = """import std.io.println;
fn int(x: i64) i64 { ret x; }
fn real(x: f64) f64 { ret x; }
fn big(x: u64) u64 { ret x; }
fn main() i32 {
    println("{} {} {} {}", int(300) as u8, int(-1) as u32, int(200) as i8, int(-40000) as i16);
    println("{} {} {}", 256 as u8, (-1) as u16, 128 as i8);
    println("{} {} {}", int(7) as f64 / 2.0, int(-9007199254740993) as f64, big(18446744073709551615) as f64);
    println("{} {}", 3 as f64, 16777217 as f32 as f64);
    println("{} {} {} {}", real(3.99) as i32, real(-3.99) as i64, real(1e20) as i32, real(-1e20) as u8);
    println("{} {} {}", real(0.0 / 0.0) as i64, real(1e19) as u64, real(-0.5) as u32);
    println("{} {} {}", 3.7 as i32, 1e20 as i16, (-2.5) as u64);
    const tenth: f32 = real(0.1) as f32;
    println("{} {}", tenth as f64 == 0.1, tenth as f64 == 0.1 as f32 as f64);
    const count: usize = 10;
    println("{}", 25.0 / (count as f64));
    0
}
"""
EXPECTED = """44 4294967295 -56 25536
0 65535 -128
3.5 -9007199254740992 1.844674e+19
3 16777216
3 -3 2147483647 0
0 10000000000000000000 0
3 32767 0
false true
2.5
"""


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
    with tempfile.TemporaryDirectory(prefix="quazi-casts-") as directory:
        root = Path(directory)
        (root / "src").mkdir()
        (root / "quazi.toml").write_text('[package]\nname="numeric_casts"\n[build]\nentry="src/main.qz"\n')
        (root / "src/main.qz").write_text(PROGRAM)
        run([str(qz), "build", *common, "-o", str(root / "native")], root)
        actual = run([str(root / "native")], root)
        assert actual == EXPECTED, f"native:\n{actual}"
        run([str(qz), "build", *common, "-i", "-o", str(root / "program.qzi")], root)
        run([str(qz), "build", str(root / "program.qzi"), *common, "-o", str(root / "portable")], root)
        actual = run([str(root / "portable")], root)
        assert actual == EXPECTED, f"QZI:\n{actual}"
    print("PASS numeric casts: narrowing, int<->float rounding/saturation, f32 rounding; native and QZI")


if __name__ == "__main__":
    main()
