#!/usr/bin/env python3
"""Check place-level partial moves: a field moves out of a structural struct
local in the local's own straight-line region; the local then destroys only
its remaining fields; whole use, a repeated field move and a conditional
move are rejected."""

import argparse
import subprocess
import tempfile
from pathlib import Path

VALID = 'import std.io.println;\nstruct Res { id: i32 }\nimpl Res { fn free(self: Res) void { println("drop {self.id}"); } }\nstruct Person { name: String, badge: Res, spare: Res, age: i32 }\nfn keep(r: Res) i32 { const id = r.id; r.free(); ret id; }\nfn main() i32 {\n    const p = Person { name: String.from("Ann"), badge: Res { id: 1 }, spare: Res { id: 2 }, age: 30 };\n    const name: String = p.name;\n    const badge_id = keep(p.badge);\n    println("{name} {badge_id} {p.age} {p.spare.id}");\n    ret 0;\n}\n'
EXPECTED = "drop 1\nAnn 1 30 2\ndrop 2\n"
INVALID = 'struct Res { id: i32 }\nstruct Person { name: String, badge: Res }\nfn take(p: Person) i32 { ret 0; }\nfn keep(r: Res) i32 { ret r.id; }\nfn whole(flag: bool) i32 {\n    const p = Person { name: String.from("a"), badge: Res { id: 1 } };\n    const n: String = p.name;\n    ret take(p);\n}\nfn twice() i32 {\n    const p = Person { name: String.from("a"), badge: Res { id: 1 } };\n    const a = keep(p.badge);\n    ret keep(p.badge);\n}\nfn branch(flag: bool) i32 {\n    const p = Person { name: String.from("a"), badge: Res { id: 1 } };\n    if flag { ret keep(p.badge); }\n    ret 0;\n}\nfn main() i32 { ret 0; }\n'
MESSAGES = [
    "use of partially moved value 'p'",
    "use of moved field `p.badge`",
    "a field moves out of a local struct only unconditionally",
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
        invalid = project(root, "invalid", INVALID)
        check = subprocess.run([qz, "check"], cwd=invalid, text=True, capture_output=True, timeout=600)
        output = check.stdout + check.stderr
        assert check.returncode != 0, output
        for message in MESSAGES:
            assert message in output, (message, output)
    print("PASS partial moves: fields move out of struct locals; remaining fields destroyed; misuse rejected")


if __name__ == "__main__":
    main()
