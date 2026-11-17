#!/usr/bin/env python3
"""Check place-level partial moves: a field moves out of a structural struct
local, also on only some paths (`if`, `else if`, early return); the local then
destroys exactly its remaining fields through its runtime drop-guard mask, and
a reassignment destroys the remains first; whole use, a repeated field move,
use after a conditional move and a move inside a loop are rejected."""

import argparse
import subprocess
import tempfile
from pathlib import Path

VALID = 'import std.io.println;\nstruct Res { id: i32 }\nimpl Res { fn free(self: Res) void { println("drop {self.id}"); } }\nstruct Person { name: String, badge: Res, spare: Res, age: i32 }\nfn keep(r: Res) i32 { const id = r.id; r.free(); ret id; }\nfn main() i32 {\n    const p = Person { name: String.from("Ann"), badge: Res { id: 1 }, spare: Res { id: 2 }, age: 30 };\n    const name: String = p.name;\n    const badge_id = keep(p.badge);\n    println("{name} {badge_id} {p.age} {p.spare.id}");\n    ret 0;\n}\n'
EXPECTED = "drop 1\nAnn 1 30 2\ndrop 2\n"
INVALID = 'struct Res { id: i32 }\nstruct Person { name: String, badge: Res }\nfn take(p: Person) i32 { ret 0; }\nfn keep(r: Res) i32 { ret r.id; }\nfn whole(flag: bool) i32 {\n    const p = Person { name: String.from("a"), badge: Res { id: 1 } };\n    const n: String = p.name;\n    ret take(p);\n}\nfn twice() i32 {\n    const p = Person { name: String.from("a"), badge: Res { id: 1 } };\n    const a = keep(p.badge);\n    ret keep(p.badge);\n}\nfn branch(flag: bool) i32 {\n    const p = Person { name: String.from("a"), badge: Res { id: 1 } };\n    if flag { keep(p.badge); }\n    ret keep(p.badge);\n}\nfn looped() i32 {\n    const p = Person { name: String.from("a"), badge: Res { id: 1 } };\n    var i: i32 = 0;\n    for (i < 2) { keep(p.badge); i++; }\n    ret 0;\n}\nfn main() i32 { ret 0; }\n'
CONDITIONAL = r"""import std.io.println;
struct Res { id: i32 }
impl Res { fn free(self: Res) void { println("drop {self.id}"); } }
struct Pair { left: Res, right: Res, tag: i32 }
fn keep(r: Res) i32 { ret r.id; }
fn branch(flag: bool) i32 {
    const p = Pair { left: Res { id: 1 }, right: Res { id: 2 }, tag: 7 };
    var got: i32 = 0;
    if flag { got = keep(p.left); }
    println("tag {}", p.tag);
    ret got;
}
fn arms(which: i32) i32 {
    const p = Pair { left: Res { id: 3 }, right: Res { id: 4 }, tag: 0 };
    if which == 0 { ret keep(p.left); }
    else if which == 1 { keep(p.right); }
    ret -1;
}
fn reassign() void {
    var p = Pair { left: Res { id: 5 }, right: Res { id: 6 }, tag: 0 };
    keep(p.left);
    p = Pair { left: Res { id: 7 }, right: Res { id: 8 }, tag: 0 };
    println("reassigned");
}
fn main() i32 {
    println("== true"); println("{}", branch(true));
    println("== false"); println("{}", branch(false));
    println("== arm0"); println("{}", arms(0));
    println("== arm1"); println("{}", arms(1));
    println("== arm2"); println("{}", arms(2));
    println("== reassign"); reassign();
    ret 0;
}
"""
CONDITIONAL_EXPECTED = """== true
drop 1
tag 7
drop 2
1
== false
tag 7
drop 2
drop 1
0
== arm0
drop 3
drop 4
3
== arm1
drop 4
drop 3
-1
== arm2
drop 4
drop 3
-1
== reassign
drop 5
drop 6
reassigned
drop 8
drop 7
"""
GENERIC = r"""import std.io.println;
struct Res { id: i32 }
impl Res { fn free(self: Res) void { println("drop {self.id}"); } }
struct Pair[A, B] { first: A, second: B }
fn keep(r: Res) i32 { ret r.id; }
fn branch(flag: bool) i32 {
    const p: Pair[Res, Res] = Pair { first: Res { id: 1 }, second: Res { id: 2 } };
    var got: i32 = 0;
    if flag { got = keep(p.first); }
    ret got;
}
fn text(flag: bool) usize {
    const p: Pair[String, String] = Pair { first: String.from("hello world"), second: String.from("second") };
    if flag {
        const s: String = p.first;
        ret s.bytes_len();
    }
    ret p.second.bytes_len();
}
fn main() i32 {
    println("{}", branch(true));
    println("{}", branch(false));
    var total: usize = 0;
    var i: i32 = 0;
    for (i < 200000) { total = total + text(i % 2 == 0); i++; }
    println("{}", total);
    ret 0;
}
"""
GENERIC_EXPECTED = "drop 1\ndrop 2\n1\ndrop 2\ndrop 1\n0\n1700000\n"
NESTED = r"""import std.io.println;
struct Res { id: i32 }
impl Res { fn free(self: Res) void { println("drop {self.id}"); } }
struct Inner { a: Res, b: Res }
struct Outer { inner: Inner, c: Res, n: i32 }
fn keep(r: Res) i32 { ret r.id; }
fn nested(flag: bool) i32 {
    const o = Outer { inner: Inner { a: Res { id: 1 }, b: Res { id: 2 } }, c: Res { id: 3 }, n: 5 };
    var got: i32 = 0;
    if flag { got = keep(o.inner.a); }
    println("b {} n {}", o.inner.b.id, o.n);
    ret got;
}
fn whole_inner() i32 {
    const o = Outer { inner: Inner { a: Res { id: 4 }, b: Res { id: 5 } }, c: Res { id: 6 }, n: 0 };
    const inner: Inner = o.inner;
    ret keep(inner.b);
}
fn main() i32 {
    println("== t"); println("{}", nested(true));
    println("== f"); println("{}", nested(false));
    println("== w"); println("{}", whole_inner());
    ret 0;
}
"""
NESTED_EXPECTED = "== t\ndrop 1\nb 2 n 5\ndrop 3\ndrop 2\n1\n== f\nb 2 n 5\ndrop 3\ndrop 2\ndrop 1\n0\n== w\ndrop 5\ndrop 4\ndrop 6\n5\n"
RECEIVER = r"""import std.io.println;
struct Item { value: i32 }
impl Item { fn free(self: Item) void { println("drop item {self.value}"); } }
struct Inner { item: Item, other: Item }
struct Outer { inner: Inner, last: Item }
impl Outer { fn take(self: Outer) Item { ret self.inner.item; } }
fn main() i32 {
    const o = Outer { inner: Inner { item: Item { value: 1 }, other: Item { value: 2 } }, last: Item { value: 3 } };
    const item: Item = o.take();
    println("got {}", item.value);
    ret 0;
}
"""
RECEIVER_EXPECTED = "drop item 3\ndrop item 2\ngot 1\ndrop item 1\n"
NESTED_INVALID = """struct Res { id: i32 }
struct Inner { a: Res, b: Res }
struct Outer { inner: Inner }
fn keep(r: Res) i32 { ret r.id; }
fn take(i: Inner) i32 { ret 0; }
fn parent() i32 {
    const o = Outer { inner: Inner { a: Res { id: 1 }, b: Res { id: 2 } } };
    keep(o.inner.a);
    ret take(o.inner);
}
fn child() i32 {
    const o = Outer { inner: Inner { a: Res { id: 1 }, b: Res { id: 2 } } };
    take(o.inner);
    ret keep(o.inner.a);
}
fn main() i32 { ret parent() + child(); }
"""
TEMPLATE = """struct Res { id: i32 }
struct Pair[A, B] { first: A, second: B }
fn keep(r: Res) i32 { ret r.id; }
fn generic[T](value: T) i32 {
    const p: Pair[Res, T] = Pair { first: Res { id: 1 }, second: value };
    ret keep(p.first);
}
fn main() i32 { ret generic(5); }
"""
MESSAGES = [
    "use of partially moved value 'p'",
    "use of moved field `p.badge`",
    "use of moved field `p.badge` (moved at",
    "a field moves out of a local struct only outside loops that do not declare it",
]


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
    with tempfile.TemporaryDirectory(prefix="quazi-partial-moves-") as directory:
        root = Path(directory)
        valid = project(root, "valid", VALID)
        result = subprocess.run([qz, "run", "--silent", "--no-incremental"], cwd=valid,
                                text=True, capture_output=True, timeout=600)
        assert result.returncode == 0, result.stdout + result.stderr
        assert result.stdout == EXPECTED, result.stdout
        conditional = project(root, "conditional", CONDITIONAL)
        result = subprocess.run([qz, "run", "--silent", "--no-incremental"], cwd=conditional,
                                text=True, capture_output=True, timeout=600)
        assert result.returncode == 0, result.stdout + result.stderr
        assert result.stdout == CONDITIONAL_EXPECTED, result.stdout
        generic = project(root, "generic", GENERIC)
        build = subprocess.run([qz, "build", "--silent", "--no-incremental", "-o",
                                str(root / "generic-bin")], cwd=generic, text=True,
                               capture_output=True, timeout=600)
        assert build.returncode == 0, build.stdout + build.stderr
        run = subprocess.run(["/usr/bin/env", "time", "-f", "%M", str(root / "generic-bin")],
                             text=True, capture_output=True, timeout=120)
        assert run.stdout == GENERIC_EXPECTED, run.stdout + run.stderr
        assert int(run.stderr.strip().splitlines()[-1]) < 20000, run.stderr
        nested = project(root, "nested", NESTED)
        result = subprocess.run([qz, "run", "--silent", "--no-incremental"], cwd=nested,
                                text=True, capture_output=True, timeout=600)
        assert result.returncode == 0, result.stdout + result.stderr
        assert result.stdout == NESTED_EXPECTED, result.stdout
        receiver = project(root, "receiver", RECEIVER)
        result = subprocess.run([qz, "run", "--silent", "--no-incremental"], cwd=receiver,
                                text=True, capture_output=True, timeout=600)
        assert result.stdout == RECEIVER_EXPECTED, result.stdout + result.stderr
        nested_invalid = project(root, "nested-invalid", NESTED_INVALID)
        check = subprocess.run([qz, "check"], cwd=nested_invalid, text=True,
                               capture_output=True, timeout=600)
        output = check.stdout + check.stderr
        assert check.returncode != 0, output
        assert "use of partially moved field `o.inner`" in output, output
        assert "use of moved field `o.inner`" in output, output
        template = project(root, "template", TEMPLATE)
        check = subprocess.run([qz, "check"], cwd=template, text=True, capture_output=True,
                               timeout=600)
        output = check.stdout + check.stderr
        assert check.returncode != 0 and "does not depend on the function's type parameters" in output, output
        invalid = project(root, "invalid", INVALID)
        check = subprocess.run([qz, "check"], cwd=invalid, text=True, capture_output=True, timeout=600)
        output = check.stdout + check.stderr
        assert check.returncode != 0, output
        for message in MESSAGES:
            assert message in output, (message, output)
    print("PASS partial moves: fields move out of struct locals, also conditionally and from "
          "concrete generic structs and nested fields; remaining fields destroyed exactly once; misuse rejected")


if __name__ == "__main__":
    main()
