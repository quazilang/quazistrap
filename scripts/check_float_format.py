#!/usr/bin/env python3
"""Check f64 formatting of values beyond the i64 range and exponent/hex literals."""

import argparse
import subprocess
import tempfile
from pathlib import Path

PROGRAM = """import std.io;
fn main() i32 {
    io.println("{} {} {} {}", 1.0e300, -2.5e20, 1.0e18, 123456789012.5);
    io.println("{:.2} {} {} {}", 6.022e23, 1.7976931348623157e308, 0.5, 0xFF);
    0
}
"""
EXPECTED = "1e+300 -2.5e+20 1e+18 123456789012.5\n6.02e+23 1.797693e+308 0.5 255\n"


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
    print("PASS float format: values >= 1e18 use scientific notation; hex/exponent literals; native and QZI")


if __name__ == "__main__":
    main()
