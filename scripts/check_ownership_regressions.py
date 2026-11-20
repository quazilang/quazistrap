#!/usr/bin/env python3
"""Ownership regressions found by the memory-safety audit, checked through
`free` hooks that print: each value is destroyed exactly once.

- `match h.e { Some(r) => keep(r), ... }` on a field of a local struct moves
  the field out; the struct's cleanup used to destroy it again (double free).
- A `@derive(Copy)` generic struct holding a non-Copy `T` moves: codegen
  used to copy `P[Res]` and destroy the payload twice."""

import argparse
import os
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

# A channel destroys the values nobody received when its last owner goes.
CHANNEL = r"""import std.io;
struct Res { id: i32 }
impl Res { fn free(self: Res) void { io.println("drop {self.id}"); } }
fn main() i32 {
    const c: Channel[Res] = Channel.new(3);
    c.send(Res { id: 1 });
    c.send(Res { id: 2 });
    c.send(Res { id: 3 });
    const got = c.receive();
    io.println("got {}", got.is_some());
    0
}
"""
CHANNEL_OUT = "got true\ndrop 1\ndrop 2\ndrop 3\n"

# A trait object owns its value wherever the value became `dyn`: locals,
# arguments, returns, fields, Array/Box elements; `self: Self` consumes it.
DYN = r"""import std.io;
struct Res { id: i64, name: String }
impl Res { fn free(self: Res) void { io.println("drop {self.id}"); } }
struct Plain { v: i64 }
trait Show { fn show(self: &Self) i64; fn eat(self: Self) i64; }
impl Show for Res { fn show(self: &Res) i64 { self.id } fn eat(self: Res) i64 { ret self.id * 10; } }
impl Show for Plain { fn show(self: &Plain) i64 { self.v } fn eat(self: Plain) i64 { ret self.v; } }
struct Holder { item: dyn Show }
enum Slot { Full(dyn Show), Empty }
fn take(x: dyn Show) i64 { ret x.show(); }
fn pick(f: bool) dyn Show {
    if (f) { ret Res { id: 4, name: String.from("d") }; }
    ret Plain { v: 5 };
}
fn main() i32 {
    var arr: Array[dyn Show] = Array.new();
    arr.push(Res { id: 1, name: String.from("a") });
    arr.push(Plain { v: 2 });
    io.println("take {}", take(Res { id: 9, name: String.from("t") }));
    const e: dyn Show = Res { id: 3, name: String.from("c") };
    io.println("eat {}", e.eat());
    const h = Holder { item: Res { id: 6, name: String.from("f") } };
    const b: Box[dyn Show] = Box.new(Res { id: 10, name: String.from("b") });
    const p = pick(true);
    const q = pick(false);
    io.println("{} {} {}", h.item.show(), p.show(), q.show());
    var r: dyn Show = Res { id: 7, name: String.from("g") };
    r = Res { id: 8, name: String.from("h") };
    io.println("temp {}", pick(true).show());
    const o: Option[dyn Show] = Option.Some(Res { id: 11, name: String.from("o") });
    match (o) { Option.Some(x) => io.println("some {}", x.show()), Option.None => io.println("none") };
    const s = Slot.Full(Res { id: 12, name: String.from("s") });
    match (s) { Slot.Full(x) => io.println("full {}", x.show()), Slot.Empty => io.println("empty") };
    const c: dyn Show = if (q.show() == 5) { Res { id: 13, name: String.from("i") } } else { Res { id: 14, name: String.from("j") } };
    io.println("end {} {}", r.show(), c.show());
    0
}
"""
# A generic constructor as an enum payload takes its type argument from the
# field (`Array.new()` stayed `Array[T]` and failed in codegen).
GENERIC_PAYLOAD = r"""import std.io;
enum E { A(Array[i64]), B }
fn main() i32 {
    var a: Array[i64] = Array.new();
    a.push(3);
    const e = E.A(a);
    const f = E.A(Array.new());
    const d: Option[Array[i64]] = Option.Some(Array.new());
    match (e) { E.A(x) => io.println("{}", x.len()), E.B => io.println("b") };
    match (d) { Option.Some(x) => io.println("d {}", x.len()), Option.None => io.println("n") };
    0
}
"""
GENERIC_PAYLOAD_OUT = "1\nd 0\n"

# A qualified unit variant has no type of its own; the declared type makes
# the binding its owner (it leaked 32 bytes per iteration).
QUALIFIED_NONE = r"""fn main() i32 {
    var i: i64 = 0;
    for (i < ITERATIONS) {
        const o: Option[String] = Option.None;
        if (o.is_some()) { ret 1; }
        i = i + 1;
    }
    0
}
"""


def peak_kib(command):
    """Peak resident memory of a child process (includes a constant
    launcher overhead, so callers compare two runs)."""
    pid = os.posix_spawn(command[0], command, os.environ)
    _, status, usage = os.wait4(pid, 0)
    assert status == 0, (command, status)
    return usage.ru_maxrss


DYN_OUT = ("drop 9\ntake 9\ndrop 3\neat 30\n6 4 5\ndrop 7\ndrop 4\ntemp 4\n"
           "some 11\ndrop 11\nfull 12\ndrop 12\nend 8 13\n"
           "drop 13\ndrop 8\ndrop 4\ndrop 10\ndrop 6\ndrop 1\n")


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
                                       ("generic_copy", GENERIC_COPY, GENERIC_COPY_OUT),
                                       ("channel", CHANNEL, CHANNEL_OUT),
                                       ("dyn", DYN, DYN_OUT),
                                       ("generic_payload", GENERIC_PAYLOAD, GENERIC_PAYLOAD_OUT)):
            root = Path(directory) / name
            (root / "src").mkdir(parents=True)
            (root / "quazi.toml").write_text(f'[package]\nname="{name}"\n[build]\nentry="src/main.qz"\n')
            (root / "src/main.qz").write_text(source)
            run([qz, "build", *common, "-o", str(root / "native")], root)
            assert run([str(root / "native")], root) == expected, (name, run([str(root / "native")], root))
            run([qz, "build", *common, "-i", "-o", str(root / "program.qzi")], root)
            run([qz, "build", str(root / "program.qzi"), *common, "-o", str(root / "portable")], root)
            assert run([str(root / "portable")], root) == expected, name
        peaks = []
        for iterations in (100000, 1100000):
            root = Path(directory) / f"qualified_none_{iterations}"
            (root / "src").mkdir(parents=True)
            (root / "quazi.toml").write_text('[package]\nname="none"\n[build]\nentry="src/main.qz"\n')
            (root / "src/main.qz").write_text(QUALIFIED_NONE.replace("ITERATIONS", str(iterations)))
            run([qz, "build", *common, "-o", str(root / "native")], root)
            peaks.append(peak_kib([str(root / "native")]))
        # A 32-byte leak per iteration would add about 31 MiB.
        assert peaks[1] - peaks[0] < 4096, f"Option.None bindings leak: peaks {peaks} KiB"
    print("PASS ownership regressions: field match moves the field once, Copy generics with owned T move, channels destroy unreceived values, trait objects destroy their values, qualified None is owned, generic payloads infer from the field; native and QZI")


if __name__ == "__main__":
    main()
