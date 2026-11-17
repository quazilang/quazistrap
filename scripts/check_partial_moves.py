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
        invalid = project(root, "invalid", INVALID)
        check = subprocess.run([qz, "check"], cwd=invalid, text=True, capture_output=True, timeout=600)
        output = check.stdout + check.stderr
        assert check.returncode != 0, output
        for message in MESSAGES:
            assert message in output, (message, output)
    print("PASS partial moves: fields move out of struct locals, also conditionally; remaining "
          "fields destroyed exactly once; misuse rejected")


if __name__ == "__main__":
    main()
