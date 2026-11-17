#!/usr/bin/env python3
"""Check recursive enum destruction: a value of an enum that contains
itself (List, Tree[T]) destroys every nested payload through a generated
drop function, consuming matches move nested parts out, and repeated
construction does not leak."""

import argparse
import subprocess
import tempfile
from pathlib import Path

SOURCE = 'import std.io.println;\nstruct Res { id: i32 }\nimpl Res { fn free(self: Res) void { println("drop {self.id}"); } }\nenum List { Cons(Res, List), Nil }\nenum Tree[T] { Node(Tree[T], T, Tree[T]), Leaf }\nfn head(list: List) i32 {\n    ret match list { List.Cons(r, rest) => r.id, List.Nil => 0 };\n}\nfn leaf() Tree[String] { ret Tree.Leaf; }\nfn count(tree: &Tree[String]) i32 { ret match tree { Tree.Node(_, _, _) => 1, Tree.Leaf => 0 }; }\nfn main() i32 {\n    const list: List = List.Cons(Res { id: 1 }, List.Cons(Res { id: 2 }, List.Nil));\n    println("head {}", head(list));\n    var total: i32 = 0;\n    var i: i32 = 0;\n    for (i < 200000) {\n        const tree: Tree[String] = Tree.Node(Tree.Node(leaf(), String.from("left-left-left"), leaf()), String.from("root-root-root"), leaf());\n        total += count(&tree);\n        i++;\n    }\n    println("trees {total}");\n    const nested: List = List.Cons(Res { id: 3 }, List.Cons(Res { id: 4 }, List.Cons(Res { id: 5 }, List.Nil)));\n    const n = match &nested { List.Cons(_, _) => 1, List.Nil => 0 };\n    println("nested {n}");\n    ret 0;\n}\n'

EXPECTED = "drop 2\ndrop 1\nhead 1\ntrees 200000\nnested 1\ndrop 3\ndrop 4\ndrop 5\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--qz", type=Path, default=Path("target/debug/qz"))
    qz = str(parser.parse_args().qz.resolve())
    with tempfile.TemporaryDirectory(prefix="quazi-recursive-enums-") as directory:
        root = Path(directory)
        (root / "src").mkdir()
        (root / "quazi.toml").write_text('[package]\nname="recursive"\n[build]\nentry="src/main.qz"\n')
        (root / "src/main.qz").write_text(SOURCE)
        build = subprocess.run([qz, "build", "--silent", "--no-incremental", "-o", str(root / "bin")],
                               cwd=root, text=True, capture_output=True, timeout=600)
        assert build.returncode == 0, build.stdout + build.stderr
        run = subprocess.run(["/usr/bin/env", "time", "-f", "%M", str(root / "bin")],
                             text=True, capture_output=True, timeout=120)
        assert run.returncode == 0, run.stdout + run.stderr
        assert run.stdout == EXPECTED, run.stdout
        assert int(run.stderr.strip().splitlines()[-1]) < 20000, run.stderr
    print("PASS recursive enums: nested payloads destroyed once, consuming match, no leak")


if __name__ == "__main__":
    main()
