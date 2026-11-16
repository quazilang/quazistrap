#!/usr/bin/env python3
"""Check enum payload ownership (D-021 В10А): a by-value `match` consumes an
owned enum, bindings own their payloads and are destroyed at the end of the
arm unless moved on, unbound payloads are destroyed, `?` and `unwrap` move
payloads, enum locals and enum struct fields destroy their active payload,
`match &value` only inspects, and using a matched value is S10. Every
resource must be destroyed exactly once."""

import argparse
import subprocess
import tempfile
from pathlib import Path

PRELUDE = """import std.io.println;
struct Res { id: i32 }
impl Res { fn free(self: Res) void { println("drop {self.id}"); } }
fn some(n: i32) Option[Res] { ret Some(Res { id: n }); }
"""

VALID = PRELUDE + """struct Holder { label: i32, slot: Option[Res] }
enum Pair { Both(Res, Res), Nothing }
fn ok(n: i32) Result[Res, Res] { if n > 0 { ret Ok(Res { id: n }); } ret Err(Res { id: 0 - n }); }
fn keep(r: Res) i32 { const id = r.id; r.free(); ret id; }
fn early(o: Option[Res]) i32 { const v = match o { Some(r) => r.id > 0, None => false }; if v { ret 1; } ret 0; }
fn tried(n: i32) Result[i32, Res] { const r = ok(n)?; ret Ok(keep(r)); }
fn main() i32 {
    println("moved"); const a = match some(1) { Some(r) => keep(r), None => 0 };
    println("wildcard"); match some(2) { Some(_) => println("wild"), None => println("none") };
    println("whole"); match some(3) { other => println("bound") };
    println("guard"); match some(4) { Some(r) if r.id > 100 => println("big"), Some(r) => println("small {r.id}"), None => println("none") };
    println("parameter"); const n = early(some(5));
    println("try ok"); const t = tried(6);
    println("try err"); const e = tried(0 - 7);
    match e { Ok(_) => println("ok"), Err(r) => println("err {r.id}") };
    println("unwrap"); const u = some(8).unwrap(); println("unwrapped {u.id}");
    println("nested"); const nested: Option[Option[Res]] = Some(some(9));
    println("field"); const h = Holder { label: 1, slot: some(10) };
    println("pair"); const pair = Pair.Both(Res { id: 11 }, Res { id: 12 }); match pair { Pair.Both(x, _) => println("first {x.id}"), Pair.Nothing => println("nothing") };
    println("borrowed"); const c = some(13); const flag = match &c { Some(_) => 1, None => 0 };
    println("end {a} {n} {flag} {h.label}");
    ret 0;
}
"""

EXPECTED = """moved
drop 1
wildcard
drop 2
wild
whole
bound
drop 3
guard
small 4
drop 4
parameter
drop 5
try ok
drop 6
try err
err 7
drop 7
unwrap
unwrapped 8
nested
field
pair
drop 12
first 11
drop 11
borrowed
end 1 1 1 1
drop 13
drop 10
drop 9
drop 8
"""

INVALID = PRELUDE + """fn main() i32 {
    const a = some(1);
    match a { Some(r) => println("{r.id}"), None => println("none") };
    ret match a { Some(_) => 1, None => 0 };
}
"""


def project(root: Path, name: str, source: str) -> Path:
    path = root / name
    (path / "src").mkdir(parents=True)
    (path / "quazi.toml").write_text(f'[package]\nname="{name}"\n[build]\nentry="src/main.qz"\n')
    (path / "src/main.qz").write_text(source)
    return path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--qz", type=Path, default=Path("target/debug/qz"))
    qz = str(parser.parse_args().qz.resolve())
    with tempfile.TemporaryDirectory(prefix="quazi-consuming-match-") as directory:
        root = Path(directory)
        valid = project(root, "valid", VALID)
        for extra in ([], ["--target", "x86_64-windows"]):
            command = [qz, "run" if not extra else "build", "--silent", "--no-incremental", *extra]
            result = subprocess.run(command, cwd=valid, text=True, capture_output=True, timeout=600)
            assert result.returncode == 0, result.stdout + result.stderr
            if not extra:
                assert result.stdout == EXPECTED, result.stdout
        invalid = project(root, "invalid", INVALID)
        result = subprocess.run([qz, "check"], cwd=invalid, text=True, capture_output=True, timeout=600)
        output = result.stdout + result.stderr
        assert result.returncode != 0 and "use of moved value 'a'" in output, output
    print("PASS consuming match: payloads destroyed exactly once; use after match rejected")


if __name__ == "__main__":
    main()
