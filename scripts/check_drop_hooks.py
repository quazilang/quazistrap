#!/usr/bin/env python3
"""Check the `free` hook contract (D-028, maintainer answer В36А): the hook
runs first, then the compiler destroys the fields it did not move out and
releases the aggregate. A by-value receiver of the type's own method may move
fields out; the hook is then skipped (also when the move happens on only some
paths) and the remaining fields are destroyed one by one. Field destructors
behind a hook (generic and contiguous fields included) are compiled. Native
and QZI builds; with --valgrind the programs also run under memcheck with
libc's allocator (no leaks, no invalid accesses). Also: `?` failure paths
destroy the right owned parameters, a field moved in one `match` arm stays
owned on the others, and capturing a partially moved value is S10."""

import argparse
import subprocess
import tempfile
from pathlib import Path

RECEIVER = 'import std.io.println;\nstruct Res { id: i32 }\nimpl Res { fn free(self: Res) void { println("drop {self.id}"); } }\nstruct Holder { name: String, a: Res, b: Res, n: i32 }\nimpl Holder {\n    fn free(self: Holder) void { println("hook {self.n}"); }\n    fn take_a(self: Holder) Res { ret self.a; }\n    fn take_maybe(self: Holder, flag: bool) i32 {\n        if flag { const r: Res = self.b; ret r.id; }\n        ret 0;\n    }\n    fn consume(self: Holder) void { println("consume {self.n}"); }\n}\nfn make(base: i32) Holder { ret Holder { name: String.from("x"), a: Res { id: base }, b: Res { id: base + 1 }, n: base * 10 }; }\nfn plain() void { const h = make(1); }\nfn take() void { const h = make(3); const r = h.take_a(); println("got {r.id}"); }\nfn maybe(flag: bool) void { const h = make(5); println("{}", h.take_maybe(flag)); }\nfn consume() void { const h = make(7); h.consume(); }\nfn main() i32 {\n    println("== plain"); plain();\n    println("== take_a"); take();\n    println("== maybe t"); maybe(true);\n    println("== maybe f"); maybe(false);\n    println("== consume"); consume();\n    ret 0;\n}\n'
RECEIVER_OUT = ("== plain\nhook 10\ndrop 2\ndrop 1\n== take_a\ndrop 4\ngot 3\ndrop 3\n"
                "== maybe t\ndrop 6\ndrop 5\n6\n== maybe f\nhook 50\ndrop 6\ndrop 5\n0\n"
                "== consume\nconsume 70\nhook 70\ndrop 8\ndrop 7\n")
HOOK_FIELDS = 'import std.io.println;\nstruct Res { id: i32 }\nimpl Res { fn free(self: Res) void { println("drop {self.id}"); } }\nstruct Only { id: i32 }\nimpl Only { fn free(self: Only) void { println("only {self.id}"); } }\n// The hook frees one field itself and moves another out; the rest follow.\nstruct Doc { title: String, keep: Res, rest: Res, only: Only, note: String }\nimpl Doc {\n    fn free(self: Doc) void {\n        self.title.free();\n        const k: Res = self.keep;\n        println("hook");\n    }\n    fn into_note(self: Doc) String { ret self.note; }\n}\nstruct Wrap[T] { value: T, tag: Res }\nimpl Wrap[T] {\n    fn free(self: Wrap[T]) void { println("wrap hook {self.tag.id}"); }\n    fn into_value(self: Wrap[T]) T { ret self.value; }\n}\nfn doc(base: i32) Doc {\n    ret Doc { title: String.from("t"), keep: Res { id: base }, rest: Res { id: base + 1 }, only: Only { id: base + 2 }, note: String.from("note") };\n}\nfn dropped() void { const d = doc(1); }\nfn noted() void { const d = doc(4); const n = d.into_note(); println("{n}"); }\nfn wrapped() void {\n    const w = Wrap { value: String.from("v"), tag: Res { id: 7 } };\n    const k = Wrap { value: Res { id: 9 }, tag: Res { id: 8 } };\n    const v: String = w.into_value();\n    println("{v}");\n}\nfn main() i32 {\n    println("== dropped"); dropped();\n    println("== noted"); noted();\n    println("== wrapped"); wrapped();\n    ret 0;\n}\n'
HOOK_FIELDS_OUT = ("== dropped\nhook\ndrop 1\nonly 3\ndrop 2\n== noted\nonly 6\ndrop 5\n"
                   "drop 4\nnote\n== wrapped\ndrop 7\nv\nwrap hook 8\ndrop 8\ndrop 9\n")
ROOTS = 'import std.io.println;\nstruct Res { id: i32 }\nimpl Res { fn free(self: Res) void { println("drop {self.id}"); } }\nstruct Cell[T] { v: T }\nimpl Cell[T] { fn free(self: Cell[T]) void { println("cell"); const x: T = self.v; } }\nstruct Bag { items: Array[Res], cell: Cell[Res], boxed: Box[Res] }\nimpl Bag { fn free(self: Bag) void { println("bag"); } }\nfn fill() Bag {\n    var items: Array[Res] = Array.new();\n    items.push(Res { id: 1 });\n    ret Bag { items: items, cell: Cell { v: Res { id: 2 } }, boxed: Box.new(Res { id: 3 }) };\n}\nfn main() i32 { const b = fill(); ret 0; }\n'
ROOTS_OUT = "bag\ndrop 3\ncell\ndrop 2\ndrop 1\n"
TRY = 'import std.io.println;\nstruct Res { id: i32 }\nimpl Res { fn free(self: Res) void { println("drop {self.id}"); } }\nstruct P { a: Res, b: Res }\nstruct H { a: Res, b: Res, n: i32 }\nimpl H {\n    fn free(self: H) void { println("hook {self.n}"); }\n    fn checked(self: H, fail: bool) Result[i32, i32] { const v: i32 = chk(fail)?; ret Ok(v); }\n    fn pick(self: H, k: i32) i32 {\n        const got: Res = match k { 0 => self.a, 1 => self.b, _ => Res { id: 0 } };\n        ret got.id;\n    }\n}\nfn chk(fail: bool) Result[i32, i32] { if fail { ret Err(1); } ret Ok(2); }\nfn param(r: Res, fail: bool) Result[i32, i32] { const v: i32 = chk(fail)?; ret Ok(v + r.id); }\nfn local(k: i32) i32 {\n    const h = P { a: Res { id: 20 }, b: Res { id: 21 } };\n    const got: Res = match k { 0 => h.a, _ => Res { id: 0 } };\n    ret got.id;\n}\nfn mk(n: i32) H { ret H { a: Res { id: n }, b: Res { id: n + 1 }, n: n * 10 }; }\nfn main() i32 {\n    println("== param"); const p = param(Res { id: 3 }, true);\n    println("== try t"); const t = mk(4).checked(true);\n    println("== try f"); const f = mk(6).checked(false);\n    println("== pick 0"); println("{}", mk(8).pick(0));\n    println("== pick 2"); println("{}", mk(10).pick(2));\n    println("== local 0"); println("{}", local(0));\n    println("== local 1"); println("{}", local(1));\n    ret 0;\n}\n'
TRY_OUT = '== param\ndrop 3\n== try t\nhook 40\ndrop 5\ndrop 4\n== try f\nhook 60\ndrop 7\ndrop 6\n== pick 0\ndrop 8\ndrop 9\n8\n== pick 2\ndrop 0\nhook 100\ndrop 11\ndrop 10\n0\n== local 0\ndrop 20\ndrop 21\n20\n== local 1\ndrop 0\ndrop 21\ndrop 20\n0\n'
CAPTURE = 'struct Res { id: i32 }\nimpl Res { fn free(self: Res) void {} }\nstruct H { a: Res, b: Res }\nimpl H {\n    fn free(self: H) void {}\n    fn peek(self: H) i32 { const r: Res = self.a; const f: fn() i32 = || self.b.id; ret f(); }\n}\nfn main() i32 { ret 0; }\n'
UPPERCASE = 'import std.io.println;\nstruct R { id: i32 }\nimpl R { fn free(self: R) void { println("free {self.id}"); } }\nstruct W[T] { v: T }\nstruct Plain { r: R }\nfn main() i32 {\n    const w: W[R] = W { v: R { id: 1 } };\n    const p: Plain = Plain { r: R { id: 2 } };\n    const o: Option[R] = Some(R { id: 3 });\n    const q = Some(R { id: 4 });\n    println("end");\n    0\n}\n'
UPPERCASE_OUT = "end\nfree 4\nfree 3\nfree 2\nfree 1\n"
PROGRAMS = (("receiver", RECEIVER, RECEIVER_OUT), ("hook_fields", HOOK_FIELDS, HOOK_FIELDS_OUT),
            ("roots", ROOTS, ROOTS_OUT), ("try_match", TRY, TRY_OUT),
            ("uppercase", UPPERCASE, UPPERCASE_OUT))


def run(command, cwd):
    result = subprocess.run(command, cwd=cwd, text=True, capture_output=True, timeout=600)
    assert result.returncode == 0, (command, result.stdout + result.stderr)
    return result.stdout


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--qz", type=Path, default=Path("target/debug/qz"))
    parser.add_argument("--valgrind", type=Path, help="valgrind executable (memcheck run)")
    args = parser.parse_args()
    qz = str(args.qz.resolve())
    common = ["--no-incremental", "--no-color", "--silent"]
    with tempfile.TemporaryDirectory(prefix="quazi-drop-hooks-") as directory:
        for name, source, expected in PROGRAMS:
            root = Path(directory) / name
            (root / "src").mkdir(parents=True)
            manifest = f'[package]\nname="{name}"\n[build]\nentry="src/main.qz"\n'
            if args.valgrind:
                manifest += '[target.x86_64-linux.link]\nlibc = true\n'
            (root / "quazi.toml").write_text(manifest)
            (root / "src/main.qz").write_text(source)
            run([qz, "build", *common, "-o", str(root / "native")], root)
            output = run([str(root / "native")], root)
            assert output == expected, (name, output)
            run([qz, "build", *common, "-i", "-o", str(root / "program.qzi")], root)
            run([qz, "build", str(root / "program.qzi"), *common, "-o", str(root / "portable")], root)
            assert run([str(root / "portable")], root) == expected, name
            if args.valgrind:
                memcheck = subprocess.run(
                    [str(args.valgrind), "--leak-check=full", "--errors-for-leak-kinds=definite",
                     "--error-exitcode=9", str(root / "native")],
                    cwd=root, text=True, capture_output=True, timeout=600)
                assert memcheck.returncode == 0, (name, memcheck.stderr)
        capture = Path(directory) / "capture"
        (capture / "src").mkdir(parents=True)
        (capture / "quazi.toml").write_text('[package]\nname="capture"\n[build]\nentry="src/main.qz"\n')
        (capture / "src/main.qz").write_text(CAPTURE)
        check = subprocess.run([qz, "check"], cwd=capture, text=True, capture_output=True, timeout=600)
        assert check.returncode != 0, check.stdout + check.stderr
        assert "cannot capture partially moved value 'self'" in check.stdout + check.stderr, check.stdout + check.stderr
    checked = ", valgrind clean" if args.valgrind else ""
    print("PASS drop hooks: hook then remaining fields, receiver field moves skip the hook, "
          f"field destructors behind hooks compiled, `?` keeps parameters, match-arm moves are conditional, partial captures rejected, all-caps type names (`R`) in generic containers destroyed; native and QZI{checked}")


if __name__ == "__main__":
    main()
