#!/usr/bin/env python3
"""Check D-014 reference results: `&place.field` addresses a field, a function
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
fn main() i32 {
    var point = Point { inner: Inner { value: 1, label: String.from("first") }, y: 2 };
    const y: &i32 = &point.y;
    const inner = point.inner_ref();
    const other = Inner { value: 5, label: String.from("other") };
    const best = larger(inner, &other);
    println("{} {} {} {}", *y, best.value, best.label, point.me().y);
    println("{} {}", point.label_ref().len(), total(point.me()));
    ret 0;
}
"""
EXPECTED = "2 5 other 2\n5 3\n"

INVALID = [
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
