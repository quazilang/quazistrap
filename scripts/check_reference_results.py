#!/usr/bin/env python3
"""Check D-014 reference results and flow-sensitive loan regions (a loan ends
after the last statement that mentions a binding derived from it): `&place.field` addresses a field, a function
may return a reference parameter or `&parameter.field`, and the caller keeps
every reference argument's root borrowed; escapes and conflicting uses are
rejected. Runs natively, from QZI without sources, and (with --wine) on
Windows."""

import argparse
import os
import subprocess
import tempfile
from pathlib import Path

SOURCE = r"""import std.io.println;
struct Inner { value: i32, label: String }
struct Point { inner: Inner, y: i32 }
impl Point {
    fn inner_ref(self: &Point) &Inner { ret &self.inner; }
    fn label_ref(self: &Point) &String { ret &self.inner.label; }
    fn me(self: &Point) &Point { ret self; }
}
fn larger(a: &Inner, b: &Inner) &Inner {
    if a.value >= b.value { ret a; }
    ret b;
}
fn total(p: &Point) i32 { ret p.y + p.inner.value; }
impl AsRef[String] for Point { fn as_ref(self: &Point) &String { ret &self.inner.label; } }
fn length[S: AsRef[String]](value: &S) usize { ret value.as_ref().len(); }
impl Inner { fn label(self: &Inner) &String { ret &self.label; } }
struct Table { values: Array[i32] }
impl Table { fn pick(self: &Table, key: &i32) &i32 { ret self.values.get_ref((*key) as usize); } }
fn chained(p: &Point) &String { ret p.inner_ref().label(); }
fn passed(p: &Point) &String { ret larger(&p.inner, &p.inner).label(); }
fn main() i32 {
    var point = Point { inner: Inner { value: 1, label: String.from("first") }, y: 2 };
    const y: &i32 = &point.y;
    const inner = point.inner_ref();
    const other = Inner { value: 5, label: String.from("other") };
    const best = larger(inner, &other);
    println("{} {} {} {}", *y, best.value, best.label, point.me().y);
    println("{} {}", point.label_ref().len(), total(point.me()));
    println("{} {}", chained(&point), passed(&point));
    // Each loan ends after the last use of the reference or view that holds it.
    const r = point.label_ref();
    println("{}", r.len());
    point.y = 7;
    const v = point.inner.label.as_str();
    const w = v;
    println("{} {}", w, point.y);
    point.y = 8;
    const k: &i32 = &point.y;
    const copied: i32 = *k;
    point.y = copied + 1;
    println("{}", point.y);
    // Array.get_ref (D-015) and AsRef (D-025): borrowed views of owners.
    var names: Array[String] = Array.new();
    names.push(String.from("alpha"));
    names.push(String.from("beta"));
    const second = names.get_ref(1);
    println("{} {}", second.len(), length(&point.inner.label));
    names.push(String.from("gamma"));
    println("{} {}", names.len(), length(&point));
    // A `self: &Self` receiver cannot keep `&key`: the loan ends with the
    // statement although `table` outlives the loop.
    var table = Table { values: Array.new() };
    table.values.push(10);
    table.values.push(20);
    var key: i32 = 0;
    var sum: i32 = 0;
    for (key < 2) {
        if (*table.pick(&key) > 0) { sum = sum + *table.pick(&key); }
        key = key + 1;
    }
    println("{}", sum);
    ret 0;
}
"""
EXPECTED = "2 5 other 2\n5 3\nfirst first\n5\nfirst 7\n9\n4 5\n3 5\n30\n"

INVALID = [
    ("struct Table { n: i32 } impl Table { fn pick(self: &Table, key: &i32) &i32 { ret key; } } "
     "fn main() i32 { const table = Table { n: 0 }; var key: i32 = 0; "
     "const picked = table.pick(&key); key = 1; ret *picked; }",
     "cannot mutate `key` while it is shared-borrowed"),
    ("struct P { y: i32 } fn f(p: P) &i32 { ret &p.y; } fn main() void {}",
     "a returned reference must be a reference parameter"),
    ("struct P { y: i32 } fn f() &i32 { const p = P { y: 1 }; ret &p.y; } fn main() void {}",
     "a returned reference must be a reference parameter"),
    ("struct P { y: i32 } impl P { fn r(self: &P) &i32 { ret &self.y; } }\n"
     "fn main() i32 { var p = P { y: 1 }; const r = p.r(); p.y = 2; ret *r; }",
     "cannot mutate `p` while it is shared-borrowed"),
    ("struct P { y: String } impl P { fn r(self: &P) &String { ret &self.y; } }\n"
     "fn take(p: P) void {}\n"
     "fn main() i32 { const p = P { y: String.from(\"a\") }; const r = p.r(); take(p); ret r.len() as i32; }",
     "cannot move `p` while it is shared-borrowed"),
    ("fn f() Option[&i32] { ret None; } fn main() void {}",
     "may return only a plain shared reference"),
    # A returned call result must derive from the caller's own reference parameters.
    ("struct I { s: String } impl I { fn r(self: &I) &String { ret &self.s; } }\n"
     "fn f(h: &I) &String { const l = I { s: String.from(\"x\") }; ret l.r(); } fn main() void {}",
     "a returned reference must be a reference parameter"),
    ("fn pick(a: &String) &String { ret a; }\n"
     "fn f(h: &String) &String { const s = String.from(\"x\"); ret pick(&s); } fn main() void {}",
     "a returned reference must be a reference parameter"),
    ("struct I { s: String } impl I { fn r(self: &I) &String { ret &self.s; } }\n"
     "fn f(h: I) &String { ret h.r(); } fn main() void {}",
     "a returned reference must be a reference parameter"),
    # Flow-sensitive loans still cover every later use, alias, shadow and back edge.
    ("struct P { y: i32 } fn main() i32 { var p = P { y: 1 }; const r: &i32 = &p.y; p.y = 2; ret *r; }",
     "cannot mutate `p` while it is shared-borrowed"),
    ("struct P { y: i32 } fn main() i32 { var p = P { y: 1 }; const r: &i32 = &p.y; const s = r; p.y = 2; ret *s; }",
     "cannot mutate `p` while it is shared-borrowed"),
    ("fn main() i32 { var value: i32 = 1; const r: &i32 = &value; if true { const r: i32 = 7; value = r; } ret *r; }",
     "cannot mutate `value` while it is shared-borrowed"),
    ("import std.io;\nfn main() void { var owner = \"first {1}\"; const view: str = owner.as_str();\n"
     "if true { const view: i32 = 0; owner = \"replaced {2}\"; } io.println(view); }",
     "cannot mutate `owner` while it is shared-borrowed"),
    ("import std.io;\nfn main() void { var owner = \"first {1}\"; var v: str = \"\";\n"
     "for i : 0..2 { owner = \"replacement {i}\"; io.println(v); v = owner.as_str(); } }",
     "cannot mutate `owner` while it is shared-borrowed"),
    ("fn main() i32 { var names: Array[String] = Array.new(); names.push(String.from(\"a\"));\n"
     "const first = names.get_ref(0); names.push(String.from(\"b\")); ret first.len() as i32; }",
     "cannot mutate `names` while it is shared-borrowed"),
    ("struct Num { n: i32, } impl AsRef[i32] for Num { fn as_ref(self: &Num) &i32 { ret &self.n; } }\n"
     "fn length[S: AsRef[String]](value: &S) usize { ret value.as_ref().len(); }\n"
     "fn main() i32 { const n = Num { n: 3 }; ret length(&n) as i32; }",
     "`Num` does not implement `AsRef[String]`"),
    # A view stored through a local `&arr!` keeps `s` borrowed for `arr`.
    ("import std.io;\nfn main() i32 { var arr: Array[str] = Array.new(); var s = String.from(\"hello\");\n"
     "const r = &arr!; r.push(s.as_str()); s = String.from(\"X\"); io.println(arr.get(0)); ret 0; }",
     "cannot mutate `s` while it is shared-borrowed"),
    # A generic AsRef impl is matched with its parameters substituted.
    ("fn first[T, S: AsRef[Array[T]]](v: &S, hint: T) &T { ret v.as_ref().get_ref(0); }\n"
     "fn main() i32 { var a: Array[i64] = Array.new(); a.push(4); const r = first(&a, String.from(\"x\")); ret r.len() as i32; }",
     "does not implement `AsRef[Array[String]]`"),
    # Exclusive access may not be minted through a shared reference.
    ("struct S { y: i32 } fn poke(p: &S) void { const q = &p.y!; *q = 99; }\n"
     "fn main() i32 { const a = S { y: 1 }; poke(&a); ret a.y; }",
     "exclusive address-of cannot reach a field through a shared reference"),
    ("struct S { y: i32 } impl S { fn poke(self: &S) void { const q = &self.y!; *q = 9; } }\n"
     "fn main() i32 { const a = S { y: 1 }; a.poke(); ret a.y; }",
     "exclusive address-of cannot reach a field through a shared reference"),
    ("struct S { x: i32, y: i32 } fn bump(x: &i32!) void { *x = 9; }\n"
     "fn main() i32 { const a = S { x: 1, y: 2 }; if true { var r = &a; bump(&r.y!); } ret a.y; }",
     "exclusive address-of cannot reach a field through a shared reference"),
]


def project(root, name, source):
    path = root / name
    (path / "src").mkdir(parents=True)
    (path / "quazi.toml").write_text(f'[package]\nname="{name}"\n[build]\nentry="src/main.qz"\n')
    (path / "src/main.qz").write_text(source)
    return path


def run(command, cwd, **kwargs):
    result = subprocess.run(command, cwd=cwd, text=True, capture_output=True, timeout=600, **kwargs)
    assert result.returncode == 0, f"{command}: {result.stdout}{result.stderr}"
    return result.stdout


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--qz", type=Path, default=Path("target/debug/qz"))
    parser.add_argument("--wine", type=Path)
    args = parser.parse_args()
    qz = str(args.qz.resolve())
    common = ["--silent", "--no-incremental"]
    with tempfile.TemporaryDirectory(prefix="quazi-reference-results-") as directory:
        root = Path(directory)
        valid = project(root, "valid", SOURCE)
        run([qz, "build", *common, "-o", str(root / "native")], valid)
        assert run([str(root / "native")], root) == EXPECTED
        run([qz, "build", *common, "-i", "-o", str(root / "program.qzi")], valid)
        (valid / "src/main.qz").unlink()
        run([qz, "build", str(root / "program.qzi"), "--silent", "-o", str(root / "portable")], root)
        assert run([str(root / "portable")], root) == EXPECTED
        targets = "native, QZI-only"
        if args.wine:
            run([qz, "build", str(root / "program.qzi"), "--silent", "--target", "x86_64-windows",
                 "-o", str(root / "program.exe")], root)
            output = run([str(args.wine), str(root / "program.exe")], root,
                         env=dict(os.environ, WINEDEBUG="-all"))
            assert output.replace("\r\n", "\n") == EXPECTED, output
            targets += ", Windows (Wine)"
        for index, (source, message) in enumerate(INVALID):
            invalid = project(root, f"invalid{index}", source)
            result = subprocess.run([qz, "check"], cwd=invalid, text=True, capture_output=True,
                                    timeout=600)
            output = result.stdout + result.stderr
            assert result.returncode != 0 and message in output, (source, output)
    print(f"PASS reference results: field addresses and parameter-derived reference returns "
          f"({targets}); {len(INVALID)} escapes and conflicts rejected")


if __name__ == "__main__":
    main()
