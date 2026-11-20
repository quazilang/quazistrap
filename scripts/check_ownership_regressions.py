#!/usr/bin/env python3
"""Ownership regressions found by the memory-safety audit, checked through
`free` hooks that print: each value is destroyed exactly once.

- `match h.e { Some(r) => keep(r), ... }` on a field of a local struct moves
  the field out; the struct's cleanup used to destroy it again (double free).
- A `@derive(Copy)` generic struct holding a non-Copy `T` moves: codegen
  used to copy `P[Res]` and destroy the payload twice."""

import argparse
import subprocess
import tempfile
from pathlib import Path

FIELD_MATCH = r"""import std.io;
struct Res { id: i32 }
impl Res { fn free(self: Res) void { io.println("drop {self.id}"); } }
struct H { e: Option[Res], other: Res }
struct W { h: H }
fn keep(r: Res) i32 { ret r.id; }
fn by_param(h: H) i32 { ret match h.e { Some(r) => keep(r), None => 0 }; }
fn main() i32 {
    const a = H { e: Some(Res { id: 1 }), other: Res { id: 2 } };
    const x: i32 = match a.e { Some(r) => r.id, None => 0 };
    io.println("x {x}");
    const b = H { e: Some(Res { id: 3 }), other: Res { id: 4 } };
    const y: i32 = match b.e { Some(r) => keep(r), None => 0 };
    io.println("y {y}");
    const w = W { h: H { e: Some(Res { id: 5 }), other: Res { id: 6 } } };
    const z: i32 = match w.h.e { Some(r) => keep(r), None => 0 };
    io.println("z {z}");
    io.println("p {}", by_param(H { e: Some(Res { id: 7 }), other: Res { id: 8 } }));
    0
}
"""
FIELD_MATCH_OUT = ("drop 1\nx 1\ndrop 3\ny 3\ndrop 5\nz 5\ndrop 7\ndrop 8\np 7\n"
                   "drop 6\ndrop 4\ndrop 2\n")

GENERIC_COPY = r"""import std.io;
struct Res { id: i32 }
impl Res { fn free(self: Res) void { io.println("drop {self.id}"); } }
@derive(Copy)
struct P[T] { v: T }
fn main() i32 {
    const p = P { v: Res { id: 1 } };
    const q = p;
    io.println("moved {}", q.v.id);
    const n = P { v: 5 };
    const m = n;
    io.println("copied {} {}", n.v, m.v);
    0
}
"""
GENERIC_COPY_OUT = "moved 1\ncopied 5 5\ndrop 1\n"


def run(command, cwd):
    result = subprocess.run(command, cwd=cwd, text=True, capture_output=True, timeout=600)
    assert result.returncode == 0, (command, result.stdout + result.stderr)
    return result.stdout


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--qz", type=Path, default=Path("target/debug/qz"))
    qz = str(parser.parse_args().qz.resolve())
    common = ["--no-incremental", "--no-color", "--silent"]
    with tempfile.TemporaryDirectory(prefix="quazi-ownership-") as directory:
        for name, source, expected in (("field_match", FIELD_MATCH, FIELD_MATCH_OUT),
                                       ("generic_copy", GENERIC_COPY, GENERIC_COPY_OUT)):
            root = Path(directory) / name
            (root / "src").mkdir(parents=True)
            (root / "quazi.toml").write_text(f'[package]\nname="{name}"\n[build]\nentry="src/main.qz"\n')
            (root / "src/main.qz").write_text(source)
            run([qz, "build", *common, "-o", str(root / "native")], root)
            assert run([str(root / "native")], root) == expected, (name, run([str(root / "native")], root))
            run([qz, "build", *common, "-i", "-o", str(root / "program.qzi")], root)
            run([qz, "build", str(root / "program.qzi"), *common, "-o", str(root / "portable")], root)
            assert run([str(root / "portable")], root) == expected, name
    print("PASS ownership regressions: field match moves the field once, Copy generics with owned T move; native and QZI")


if __name__ == "__main__":
    main()
