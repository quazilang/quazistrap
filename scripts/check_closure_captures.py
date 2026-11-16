#!/usr/bin/env python3
"""Check closure captures (D-023 В14Г): a closure captures owned values by
move, borrows them while it runs, and destroys them with its environment;
moving a capture out is only possible in a one-shot `qo` closure, and the
outer variable is moved."""

import argparse
import subprocess
import tempfile
from pathlib import Path

VALID = """import std.io.println;
struct Res { id: i32 }
impl Res { fn free(self: Res) void { println("drop {self.id}"); } fn get(self: &Res) i32 { ret self.id; } }
fn apply(f: fn(i32) i32, x: i32) i32 { ret f(x); }
fn make_adder(base: Res) fn(i32) i32 { ret |x| x + base.get(); }
fn main() i32 {
    const r = Res { id: 10 };
    const name = String.from("quazi");
    const add: fn(i32) i32 = |x| x + r.get() + (name.len() as i32);
    println("{} {}", add(1), add(2));
    println("{}", apply(add, 3));
    const adder = make_adder(Res { id: 100 });
    println("adder {}", adder(5));
    var total: i32 = 0;
    var i: i32 = 0;
    for (i < 100000) {
        const owned = String.from("abcdefghijklmnopqrstuvwxyz");
        const size: fn() i32 = || owned.len() as i32;
        total += size();
        i++;
    }
    println("total {total}");
    println("end of main");
    ret 0;
}
"""

EXPECTED = "16 17\ndrop 10\n18\nadder 105\ntotal 2600000\nend of main\ndrop 100\n"

INVALID = """struct Res { id: i32 }
fn consume(r: Res) i32 { ret r.id; }
fn main() i32 {
    const r = Res { id: 1 };
    const f: fn() i32 = || consume(r);
    const again = r.id;
    ret f() + again;
}
"""


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
    with tempfile.TemporaryDirectory(prefix="quazi-closures-") as directory:
        root = Path(directory)
        valid = project(root, "valid", VALID)
        build = subprocess.run([qz, "build", "--silent", "--no-incremental", "-o", str(root / "valid.bin")],
                               cwd=valid, text=True, capture_output=True, timeout=600)
        assert build.returncode == 0, build.stdout + build.stderr
        run = subprocess.run(["/usr/bin/env", "time", "-f", "%M", str(root / "valid.bin")],
                             text=True, capture_output=True, timeout=120)
        assert run.returncode == 0, run.stdout + run.stderr
        assert run.stdout == EXPECTED, run.stdout
        max_rss_kb = int(run.stderr.strip().splitlines()[-1])
        assert max_rss_kb < 20000, f"captured owners leak: max RSS {max_rss_kb} KB"
        windows = subprocess.run([qz, "build", "--silent", "--no-incremental", "--target", "x86_64-windows"],
                                 cwd=valid, text=True, capture_output=True, timeout=600)
        assert windows.returncode == 0, windows.stdout + windows.stderr
        invalid = project(root, "invalid", INVALID)
        check = subprocess.run([qz, "check"], cwd=invalid, text=True, capture_output=True, timeout=600)
        output = check.stdout + check.stderr
        assert check.returncode != 0, output
        assert "cannot move captured `r` out of a closure that can run more than once" in output, output
        assert "use of moved value 'r'" in output, output
    print("PASS closure captures: owned captures borrowed and destroyed with the closure; move-out and reuse rejected")


if __name__ == "__main__":
    main()
