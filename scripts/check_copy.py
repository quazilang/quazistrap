#!/usr/bin/env python3
"""Check `Copy` (D-025): generic parameters move unless `T: Copy`;
`@derive(Copy)` structs (also generic and nested) are duplicated wherever a
place is consumed, so copies stay independent and every shell is released;
non-Copy fields, `free` hooks and non-Copy bound arguments are rejected."""

import argparse
import subprocess
import tempfile
from pathlib import Path

VALID = r"""import std.io.println;
@derive(Copy)
struct Point { x: i32, y: i32, }
@derive(Copy)
struct Line { a: Point, b: Point, }
@derive(Copy)
struct Pair[T] { first: T, second: T, }
fn bump(p: Point) Point { var q = p; q.x = q.x + 100; ret q; }
fn twice[T: Copy](value: T) Pair[T] { ret Pair { first: value, second: value }; }
fn main() i32 {
    var p = Point { x: 1, y: 2 };
    var q = p;
    q.x = 10;
    const r = bump(p);
    println("{} {} {}", p.x, q.x, r.x);
    var line = Line { a: p, b: p };
    line.a.x = 50;
    var other = line;
    other.b.y = 70;
    println("{} {} {} {}", p.x, line.a.x, line.b.y, other.b.y);
    const pair = twice(p);
    println("{} {}", pair.first.x, pair.second.y);
    var total: i64 = 0;
    var i: i32 = 0;
    for (i < 300000) {
        var c = line;
        c.a.x = i;
        const d = c;
        total = total + d.a.x as i64 + line.a.x as i64;
        i++;
    }
    println("{}", total);
    ret 0;
}
"""
EXPECTED = "1 10 101\n1 50 2 70\n1 2\n45014850000\n"
INVALID = r"""@derive(Copy)
struct Named { name: String, }
struct Res { id: i32 }
impl Res { fn free(self: Res) void {} }
impl Copy for Res {}
@derive(Copy)
struct Pair[T] { first: T, }
fn twice[T: Copy](value: T) T { ret value; }
fn take(p: Pair[String]) void {}
fn main() i32 {
    const s = twice(String.from("x"));
    const p = Pair { first: String.from("y") };
    take(p);
    take(p);
    ret 0;
}
"""
MESSAGES = [
    "`Res` has a `free` hook, so it cannot be `Copy`",
    "`Named` cannot be `Copy`: field `name` has type `String`",
    "`String` is not `Copy`, required by `T` of `twice`",
    "use of moved value 'p'",
]
GENERIC = """fn take(value: String) void {}
fn twice[T](value: T) void { take2(value); take2(value); }
fn take2[T](value: T) void {}
fn main() i32 { twice(String.from("x")); ret 0; }
"""


def project(root, name, source):
    path = root / name
    (path / "src").mkdir(parents=True)
    (path / "quazi.toml").write_text(f'[package]\nname="{name}"\n[build]\nentry="src/main.qz"\n')
    (path / "src/main.qz").write_text(source)
    return path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--qz", type=Path, default=Path("target/debug/qz"))
    qz = str(parser.parse_args().qz.resolve())
    with tempfile.TemporaryDirectory(prefix="quazi-copy-") as directory:
        root = Path(directory)
        valid = project(root, "valid", VALID)
        build = subprocess.run([qz, "build", "--silent", "--no-incremental", "-o", str(root / "bin")],
                               cwd=valid, text=True, capture_output=True, timeout=600)
        assert build.returncode == 0, build.stdout + build.stderr
        run = subprocess.run(["/usr/bin/env", "time", "-f", "%M", str(root / "bin")], text=True,
                             capture_output=True, timeout=120)
        assert run.stdout == EXPECTED, run.stdout + run.stderr
        assert int(run.stderr.strip().splitlines()[-1]) < 20000, run.stderr
        for name, source, messages in [("invalid", INVALID, MESSAGES),
                                       ("generic", GENERIC, ["use of moved value 'value'"])]:
            path = project(root, name, source)
            check = subprocess.run([qz, "check"], cwd=path, text=True, capture_output=True, timeout=600)
            output = check.stdout + check.stderr
            assert check.returncode != 0, output
            for message in messages:
                assert message in output, (message, output)
    print("PASS copy: generic values move without T: Copy; derived Copy structs (generic, nested) "
          "copy independently without leaks; misuse rejected")


if __name__ == "__main__":
    main()
