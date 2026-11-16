#!/usr/bin/env python3
"""Check Display variadics (D-024, maintainer answer В11В): formatting
functions are ordinary Quazi functions taking `...args: Display`, values of
any displayable type are passed as their text, `...args` forwards a variadic
parameter, and the removed `@format` attribute is rejected."""

import argparse
import subprocess
import tempfile
from pathlib import Path

VALID = """import std.io;
import std.io.println;
fn shout(template: str, ...args: Display) String { ret format(template, ...args); }
fn sum(first: i32, ...rest: i32) i32 { var total = first; for value : rest { total += value; } ret total; }
fn total(...values: i32) i32 { ret sum(0, ...values); }
fn count(...values: Display) usize { ret values.len(); }
struct Point { x: i32, y: i32 }
impl Display for Point { fn to_string(self: &Point) String { ret "({self.x}, {self.y})"; } }
fn main() i32 {
    const p = Point { x: 1, y: 2 };
    println("point {} {p} {}", p, p.x);
    const n: i32 = 42;
    const s = String.from("abc");
    const pi: f64 = 3.14159;
    println("plain");
    println("{} and {} ({})", n, s, true);
    println("named {n} {s} {pi:.2} {n:X}");
    println("braces {{}} {}", 7);
    const text = shout("<{}|{}>", "x", n);
    println(text.as_str());
    const raw = String.from("verbatim {{}} {{");
    println(raw.as_str());
    io.stdout().writelnf("method {} {}", 1, 2);
    println("{} {} {}", total(1, 2, 3), total(), count(n, s, pi, false));
    ret 0;
}
"""

EXPECTED = """point (1, 2) (1, 2) 1
plain
42 and abc (true)
named 42 abc 3.14 2A
braces {} 7
<x|42>
verbatim {} {
method 1 2
6 0 4
"""

INVALID = [
    ("@format\nfn old(template: str, ...args: any) void {}\nfn main() i32 { ret 0; }\n",
     "`@format` is removed"),
    ("fn first(...values: i32) i32 { ret 0; }\nfn main() i32 { const x: i32 = 1; ret first(...x); }\n",
     "`...` forwards only a variadic parameter"),
    ("struct Opaque { value: i32 }\nfn show(...args: Display) void {}\n"
     "fn main() i32 { show(Opaque { value: 1 }); ret 0; }\n",
     "format requires a numeric value"),
]


def project(root: Path, name: str, source: str) -> Path:
    path = root / name
    (path / "src").mkdir(parents=True)
    (path / "quazi.toml").write_text(f'[package]\nname="{name}"\n[build]\nentry="src/main.qz"\n')
    (path / "src/main.qz").write_text(source)
    return path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--qz", type=Path, default=Path("target/debug/qz"))
    qz = str(parser.parse_args().qz.resolve())
    with tempfile.TemporaryDirectory(prefix="quazi-display-variadics-") as directory:
        root = Path(directory)
        valid = project(root, "valid", VALID)
        result = subprocess.run([qz, "run", "--silent", "--no-incremental"], cwd=valid,
                                text=True, capture_output=True, timeout=600)
        assert result.returncode == 0, result.stdout + result.stderr
        assert result.stdout == EXPECTED, result.stdout
        windows = subprocess.run([qz, "build", "--silent", "--no-incremental", "--target",
                                  "x86_64-windows"], cwd=valid, text=True, capture_output=True,
                                 timeout=600)
        assert windows.returncode == 0, windows.stdout + windows.stderr
        for index, (source, message) in enumerate(INVALID):
            invalid = project(root, f"invalid{index}", source)
            result = subprocess.run([qz, "check"], cwd=invalid, text=True,
                                    capture_output=True, timeout=600)
            output = result.stdout + result.stderr
            assert result.returncode != 0 and message in output, (source, output)
    print(f"PASS display variadics: formatting, forwarding, Windows build; {len(INVALID)} invalid uses rejected")


if __name__ == "__main__":
    main()
