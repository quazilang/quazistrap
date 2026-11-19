#!/usr/bin/env python3
"""Check that struct storage shells are released: a loop creating 20000
scalar-only structs (one moved into a function, one dropped locally) keeps
the process's mapped pages flat (Linux), and nested structs release both
shells. Assigning an owned field destroys the value it replaces."""

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


GENERIC = r"""import std.io;
import std.fs;

pub struct Pair[T] { left: T, right: T, }
pub struct Named[T] { label: String, value: T, }

fn sum(p: Pair[i64]) i64 { p.left + p.right }

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
        const p = Pair { left: 1 as i64, right: 2 as i64 };
        total = total + sum(p);
        const n = Named { label: "abc".to_string(), value: 4 as i64 };
        total = total + n.value + n.label.len() as i64;
    }
    io.println("total {} before {} after {}", total, before, mapped_pages());
    0
}
"""


ENUMS = r"""import std.io;
import std.fs;

pub enum Shape { Circle(i64), Square(i64), Empty, }

fn area(s: Shape) i64 {
    match s { Shape.Circle(r) => 3 * r * r, Shape.Square(a) => a * a, Shape.Empty => 0, }
}

fn maybe(n: i64) Option[i64] { if (n % 2 == 0) { ret Some(n); } None }

fn checked(n: i64) Result[i64, i64] { if (n >= 0) { ret Ok(n); } Err(n) }

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
        const n: i64 = i as i64;
        total = total + area(Shape.Square(2)) + area(Shape.Empty);
        const m = maybe(n);
        if (m.is_some()) { total = total + 1; }
        total = total + checked(n).unwrap_or(0) % 2;
        const text = maybe(n);
        total = total + text.unwrap_or(1);
    }
    io.println("total {} before {} after {}", total, before, mapped_pages());
    0
}
"""


# Assigning an owned field destroys the value it replaces: arrays through a
# local, a `&T!` method (generic struct) and an owned struct field with a
# `free` hook (drop order printed once).
FIELDS = r"""import std.io;
import std.fs;

struct Res { id: i32 }
impl Res { fn free(self: Res) void { io.println("drop {self.id}"); } }
struct Pair { left: Res, right: Res }
struct Holder[T] { items: Array[T] }
impl Holder[T] {
    fn reset(self: &Holder[T]!) void { self.items = Array.empty(); }
}
struct W { a: Array[usize] }

fn mapped_pages() u64 {
    const result = fs.read_to_string("/proc/self/statm");
    if (result.is_err()) { ret 0; }
    const text: String = result.unwrap();
    const end: usize = text.as_str().find(" ").unwrap();
    const field = text[0:end];
    ret field.as_str().parse_u64().unwrap();
}

fn main() i32 {
    var p = Pair { left: Res { id: 1 }, right: Res { id: 2 } };
    p.right = Res { id: 3 };
    p.left = Res { id: 4 };
    io.println("replaced");
    const before = mapped_pages();
    var round: usize = 0;
    for (round < 100000) {
        var first: Array[usize] = Array.empty();
        first.push(7);
        var w = W { a: first };
        w.a = Array.empty();
        var h: Holder[i64] = Holder { items: Array.empty() };
        h.items.push(5);
        h.reset();
        round = round + 1;
    }
    io.println("before {} after {}", before, mapped_pages());
    0
}
"""


# Inside a `free` hook, a field consumed and then assigned again must not
# have its (already destroyed) old value destroyed by the assignment.
HOOK = r"""import std.io;

struct Res { id: i32 }
impl Res { fn free(self: Res) void { io.println("drop {self.id}"); } }
fn take(r: Res) void { io.println("took {r.id}"); }
struct P { r: Res, s: String }
impl P {
    fn free(self: P) void {
        take(self.r);
        self.r = Res { id: 2 };
        self.s.free();
        self.s = String.from("newvalue");
        const other: String = String.from("XXXXXXXX");
        io.println("r {} s {} other {}", self.r.id, self.s, other);
        const keep: Res = self.r;
    }
}
fn main() i32 {
    var p = P { r: Res { id: 1 }, s: String.from("oldvalue") };
    io.println("end");
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
    (case / "src/main.qz").write_text(GENERIC)
    output = invoke([str(qz), "run", "--no-incremental", "--no-color", "--silent"], case)
    match = re.search(r"total (\d+) before (\d+) after (\d+)", output)
    assert match, output
    generic_total, generic_before, generic_after = (int(value) for value in match.groups())
    assert generic_total == 200000 and generic_after - generic_before < 1000, output
    (case / "src/main.qz").write_text(ENUMS)
    output = invoke([str(qz), "run", "--no-incremental", "--no-color", "--silent"], case)
    match = re.search(r"total (\d+) before (\d+) after (\d+)", output)
    assert match, output
    enum_total, enum_before, enum_after = (int(value) for value in match.groups())
    assert enum_total == 100100000 and enum_after - enum_before < 1000, output
    (case / "src/main.qz").write_text(FIELDS)
    output = invoke([str(qz), "run", "--no-incremental", "--no-color", "--silent"], case)
    assert output.startswith("drop 2\ndrop 1\nreplaced\n"), output
    match = re.search(r"before (\d+) after (\d+)", output)
    assert match, output
    field_before, field_after = (int(value) for value in match.groups())
    # 200000 leaked arrays would add thousands of pages.
    assert field_after - field_before < 200, output
    (case / "src/main.qz").write_text(HOOK)
    output = invoke([str(qz), "run", "--no-incremental", "--no-color", "--silent"], case)
    assert output == "end\ntook 1\ndrop 1\nr 2 s newvalue other XXXXXXXX\ndrop 2\n", output
    print(f"PASS struct shells: 40000 structs ({before} -> {after} pages), "
          f"40000 generic structs incl. String fields ({generic_before} -> {generic_after}), "
          f"100000 enum values ({enum_before} -> {enum_after}), replaced owned fields destroyed "
          f"({field_before} -> {field_after})")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--qz", type=Path, default=Path("target/debug/qz"))
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="quazi-shells-") as directory:
        check(args.qz.resolve(), Path(directory))


if __name__ == "__main__":
    main()
