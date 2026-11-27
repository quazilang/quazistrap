#!/usr/bin/env python3
"""Check fixed arrays: nested arrays are laid out flat and index correctly
(literal and dynamic indices, reads, writes, compound assignment, bounds);
owned call results move into array literals (they were destroyed as
temporaries: use after free); arrays of owned elements are destroyed with
their owner (they leaked) and once when moved; iterating a fixed array of
owned elements by value is rejected (it read freed memory); fixed arrays are stored inline in struct fields (reads,
writes through the struct and through `&S!`). Native and QZI;
with --valgrind also under memcheck with libc's allocator."""

import argparse
import subprocess
import tempfile
from pathlib import Path

NESTED = r'''import std.io.println;
fn sum(m: [[i64; 3]; 3]) i64 {
    var t: i64 = 0;
    var i: usize = 0;
    for (i < 3) { var j: usize = 0; for (j < 3) { t = t + m[i][j]; j = j + 1; } i = i + 1; }
    ret t;
}
fn main() i32 {
    var m: [[i64; 3]; 3] = [[1, 2, 3], [4, 5, 6], [7, 8, 9]];
    var i: usize = 2;
    var j: usize = 1;
    println("{} {} {}", m[2][1], m[i][j], m[1][2]);
    m[i][j] = 80;
    m[0][0] = 10;
    m[i][0] += 3;
    println("{} {} {} {}", m[2][1], m[0][0], m[2][0], sum(m));
    0
}
'''
NESTED_OUT = "8 8 6\n80 10 10 129\n"
OWNED = r'''import std.io.println;
struct R { id: i32 }
impl R { fn free(self: R) void { println("drop {self.id}"); } }
fn ms(v: str) String { String.from(v) }
fn take(a: [R; 2]) void { println("took"); }
fn main() i32 {
    const t = String.from("t");
    const s: [String; 3] = [ms("one"), String.from("p"), t];
    const filler = String.from("ZZZZZZZZ");
    println("[{}] [{}] [{}] {}", s[0], s[1], s[2], filler);
    const a: [R; 2] = [R { id: 1 }, R { id: 2 }];
    const b: [R; 2] = [R { id: 3 }, R { id: 4 }];
    take(b);
    println("{}", a[1].id);
    0
}
'''
OWNED_OUT = "[one] [p] [t] ZZZZZZZZ\ntook\ndrop 3\ndrop 4\n2\ndrop 1\ndrop 2\n"
FIELDS = r'''import std.io.println;
struct S { head: i64, pair: [i64; 2], tail: i64 }
fn make(x: i64) S { ret S { head: x, pair: [x + 1, x + 2], tail: x + 3 }; }
fn bump(s: &S!, i: usize) void { s.pair[i] += 100; s.tail = s.pair[0]; }
fn main() i32 {
    var s = make(10);
    println("{} {} {} {}", s.head, s.pair[0], s.pair[1], s.tail);
    s.pair[1] = 50;
    s.pair = [7, 8];
    var i: usize = 1;
    println("{} {}", s.pair[i], s.tail);
    bump(&s!, 1);
    bump(&s!, 0);
    const p: [i64; 2] = s.pair;
    println("{} {} {}", p[0], p[1], s.tail);
    0
}
'''
FIELDS_OUT = "10 11 12 13\n8 13\n107 108 107\n"
VARIADIC = 'import std.io.println;\nfn by_value(...pairs: [i64; 2]) i64 {\n    var t: i64 = 0;\n    for p : pairs { t = t + p[0] * 10 + p[1]; }\n    ret t;\n}\nfn by_index(...pairs: [i64; 2]) i64 {\n    var t: i64 = 0;\n    var i: usize = 0;\n    for (i < pairs.len()) { t = t + pairs[i][1]; i = i + 1; }\n    ret t;\n}\nfn main() i32 {\n    println("{} {}", by_value([1, 2], [3, 4], [5, 6]), by_index([1, 2], [3, 4], [5, 6]));\n    println("{}", by_value());\n    const m: [[i64; 2]; 2] = [[1, 2], [3, 4]];\n    for row : m { println("{} {}", row[0], row[1]); }\n    0\n}\n'
VARIADIC_OUT = "102 12\n0\n1 2\n3 4\n"
ENUMS = 'import std.io.println;\nimport std.thread;\nenum Shape { Pair([i64; 2], i64), Single(i64) }\nfn first(v: Option[[i64; 2]]) i64 {\n    match v { Some(p) => p[0] + p[1], None => -1 }\n}\nfn pick(flag: bool) Result[[i64; 2], i64] {\n    if flag { ret Ok([3, 4]); }\n    ret Err(9);\n}\nfn chain(flag: bool) Result[i64, i64] {\n    const p = pick(flag)?;\n    ret Ok(p[0] * 10 + p[1]);\n}\nfn triple() [i64; 3] { ret [7, 8, 9]; }\nfn sum(p: [i64; 3], k: i64) i64 { ret p[0] + p[1] + p[2] + k; }\nfn call[T](f: fn() T) T { ret f(); }\nfn main() i32 {\n    const s = Shape.Pair([5, 6], 7);\n    match s {\n        Shape.Pair(a, b) => println("{} {} {}", a[0], a[1], b),\n        Shape.Single(x) => println("{}", x),\n    };\n    println("{} {}", first(Some([1, 2])), first(None));\n    println("{} {}", chain(true).unwrap(), chain(false).unwrap_err());\n    const f: fn() [i64; 3] = triple;\n    const v = f();\n    const w = call(triple);\n    const g: fn([i64; 3], i64) i64 = sum;\n    println("{} {} {} {}", v[2], w[0], g([1, 2, 3], 10), thread.spawn(triple).unwrap().join().unwrap()[1]);\n    const ch: Channel[[i64; 3]] = Channel.new(2);\n    ch.send([4, 5, 6]);\n    match ch.receive() { Some(a) => println("{} {} {}", a[0], a[1], a[2]), None => println("none") };\n    0\n}\n'
ENUMS_OUT = "5 6 7\n3 -1\n34 9\n9 7 16 8\n4 5 6\n"
ITERATE = r'''struct P { x: i64, y: i64, }
fn main() i32 {
    const c: [P; 2] = [P { x: 1, y: 10 }, P { x: 2, y: 20 }];
    for p : c { const x = p.x; }
    0
}
'''


def run(command, cwd, expected=0):
    result = subprocess.run(command, cwd=cwd, text=True, capture_output=True, timeout=600)
    assert result.returncode == expected, (command, result.stdout + result.stderr)
    return result.stdout + result.stderr


def project(root, name, source, libc):
    path = root / name
    (path / "src").mkdir(parents=True)
    manifest = f'[package]\nname="{name}"\n[build]\nentry="src/main.qz"\n'
    if libc:
        manifest += '[target.x86_64-linux.link]\nlibc = true\n'
    (path / "quazi.toml").write_text(manifest)
    (path / "src/main.qz").write_text(source)
    return path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--qz", type=Path, default=Path("target/debug/qz"))
    parser.add_argument("--valgrind", type=Path, help="valgrind executable (memcheck run)")
    args = parser.parse_args()
    qz = str(args.qz.resolve())
    common = ["--no-incremental", "--no-color", "--silent"]
    with tempfile.TemporaryDirectory(prefix="quazi-fixed-arrays-") as directory:
        root = Path(directory)
        for name, source, expected in (("nested", NESTED, NESTED_OUT), ("owned", OWNED, OWNED_OUT),
                                       ("fields", FIELDS, FIELDS_OUT),
                                       ("variadic", VARIADIC, VARIADIC_OUT),
                                       ("enums", ENUMS, ENUMS_OUT)):
            path = project(root, name, source, bool(args.valgrind))
            run([qz, "build", *common, "-o", str(path / "native")], path)
            assert run([str(path / "native")], path) == expected, name
            run([qz, "build", *common, "-i", "-o", str(path / "program.qzi")], path)
            run([qz, "build", str(path / "program.qzi"), *common, "-o", str(path / "portable")], path)
            assert run([str(path / "portable")], path) == expected, name
            if args.valgrind:
                run([str(args.valgrind), "--leak-check=full", "--errors-for-leak-kinds=definite",
                     "--error-exitcode=9", str(path / "native")], path)
        path = project(root, "iterate", ITERATE, False)
        output = run([qz, "check"], path, expected=1)
        assert "cannot iterate over a fixed array of `P` by value" in output, output
    checked = ", valgrind clean" if args.valgrind else ""
    print("PASS fixed arrays: nested layout and indexing, owned literal elements, "
          f"destruction, owning by-value iteration rejected, inline array fields, multi-slot variadics and loops, enum payloads, fn-value calls, raw derefs; native and QZI{checked}")


if __name__ == "__main__":
    main()
