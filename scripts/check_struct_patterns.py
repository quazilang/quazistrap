#!/usr/bin/env python3
"""Check struct destructuring in match patterns: shorthand and nested
fields, literal sub-patterns, `..`, generic structs, structs inside enum
payloads, borrowed scrutinees, owned String fields (with and without a
`free` hook) and the diagnostics, natively and from source-removed QZI."""

import argparse
import subprocess
import tempfile
from pathlib import Path

CASES = {
    "basic": (r"""import std.io;

pub struct Point { x: i64, y: i64, }
pub struct Line { from: Point, to: Point, tag: i64, }

fn describe(p: Point) i64 {
    match p {
        Point { x: 0, y: 0 } => 1000,
        Point { x: 0, y } => 2000 + y,
        Point { x, y: 0 } => 3000 + x,
        Point { x, y } => x * 10 + y,
    }
}

fn length(line: Line) i64 {
    match line {
        Line { from: Point { x: a, y: b }, to: Point { x: c, .. }, .. } => a + b + c,
    }
}

fn main() i32 {
    io.println("{} {} {} {}", describe(Point { x: 0, y: 0 }), describe(Point { x: 0, y: 7 }),
        describe(Point { x: 5, y: 0 }), describe(Point { x: 3, y: 4 }));
    io.println("{}", length(Line { from: Point { x: 1, y: 2 }, to: Point { x: 30, y: 40 }, tag: 9 }));
    0
}
""", "1000 2007 3005 34\n33"),
    "generic": (r"""import std.io;

pub struct Point { x: i64, y: i64, }
pub struct Pair[T] { left: T, right: T, }

fn sum(p: Pair[i64]) i64 {
    match p { Pair { left, right } => left + right, }
}

fn first(value: Option[Point]) i64 {
    match value {
        Some(Point { x, y }) => x + y,
        None => -1,
    }
}

fn main() i32 {
    io.println("{} {} {}", sum(Pair { left: 4 as i64, right: 5 as i64 }), first(Some(Point { x: 2, y: 3 })), first(None));
    0
}
""", "9 5 -1"),
    "owned": (r"""import std.io;

pub struct Job { id: i64, label: String, }

fn size(job: Job) i64 {
    match job { Job { id, label } => id + label.len() as i64, }
}

fn borrowed(job: &Job) i64 {
    match job { Job { id, .. } => id, }
}

fn main() i32 {
    var total: i64 = 0;
    for i : 0..20000 {
        const job = Job { id: 1, label: "abcd".to_string() };
        total = total + borrowed(&job);
        total = total + size(job);
    }
    io.println("total {}", total);
    0
}
""", "total 120000"),
    "hook": (r"""import std.io;

pub struct Owned { id: i64, label: String, }

impl Owned {
    pub fn free(self: Owned) void { self.label.free(); }
}

fn size(item: Owned) i64 {
    match item { Owned { id, label } => id + label.len() as i64, }
}

fn main() i32 {
    var total: i64 = 0;
    for i : 0..20000 {
        const item = Owned { id: 1, label: "abcd".to_string() };
        total = total + size(item);
    }
    io.println("total {}", total);
    0
}
""", "total 100000"),
}

ERRORS = r"""pub struct Point { x: i64, y: i64, }
pub struct Other { x: i64, }
fn f(p: Point) i64 {
    match p { Point { x } => x, }
}
fn g(p: Point) i64 {
    match p { Point { z, .. } => z, }
}
fn h(p: Point) i64 {
    match p { Other { x } => x, }
}
fn main() i32 { 0 }
"""


def run(command, root):
    return subprocess.run(command, cwd=root, text=True, stdout=subprocess.PIPE,
                          stderr=subprocess.STDOUT, timeout=300)


def invoke(command, root):
    result = run(command, root)
    assert result.returncode == 0, f"{command!r} returned {result.returncode}:\n{result.stdout}"
    return result.stdout


def project(root, name, source):
    case = root / name
    (case / "src").mkdir(parents=True)
    (case / "quazi.toml").write_text(f'[package]\nname="{name}"\n[build]\nentry="src/main.qz"\n')
    (case / "src/main.qz").write_text(source)
    return case


def check(qz, root):
    common = ["--no-incremental", "--no-color", "--no-unicode", "--silent"]
    for name, (source, expected) in CASES.items():
        case = project(root, name, source)
        binary = case / "native"
        invoke([str(qz), "build", *common, "-o", str(binary)], case)
        assert expected in invoke([str(binary)], case), name
        artifact = case / "program.qzi"
        invoke([str(qz), "build", *common, "-i", "-o", str(artifact)], case)
        (case / "src/main.qz").unlink()
        portable = case / "portable"
        invoke([str(qz), "build", str(artifact), *common, "-o", str(portable)], root)
        assert expected in invoke([str(portable)], case), name
    case = project(root, "errors", ERRORS)
    output = run([str(qz), "check"], case).stdout
    for message in ("does not mention field(s) `y`", "has no field `z`",
                    "struct pattern `Other` does not match the scrutinee type `Point`"):
        assert message in output, output
    print("PASS struct patterns: 4 programs native/source-removed QZI, 3 diagnostics")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--qz", type=Path, default=Path("target/debug/qz"))
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="quazi-struct-patterns-") as directory:
        check(args.qz.resolve(), Path(directory))


if __name__ == "__main__":
    main()
