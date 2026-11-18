#!/usr/bin/env python3
"""Check that unbounded recursion reports a stack overflow (Linux crash
handler on an alternate signal stack, exit status 134) instead of dying
with a bare SIGSEGV, and that other faults keep the generic crash report
(exit status 101). Native and QZI."""

import argparse
import os
import subprocess
import tempfile
from pathlib import Path

RECURSION = """fn deep(n: i64) i64 { ret deep(n + 1) + 1; }
fn main() i32 { ret deep(0) as i32; }
"""
NESTED_PANIC = """@panic_handler
fn handler(info: PanicInfo) ! { panic("inner failure"); }
fn main() i32 { panic("outer failure"); }
"""
NULL_READ = """fn main() i32 {
    var p: *i64 = 0;
    unsafe { ret (*p) as i32; }
}
"""
NAMED_FRAMES = """fn inner(v: i32) i32 { if v > 2 { panic("bad value {}", v); } ret v; }
fn middle(v: i32) i32 { ret inner(v + 1) * 2; }
fn main() i32 { ret middle(5); }
"""


def build(qz, root, source, qzi, environment=None):
    (root / "src/main.qz").write_text(source)
    common = ["--no-incremental", "--no-color", "--no-unicode", "--silent"]
    binary = root / "program"
    if qzi:
        steps = [[str(qz), "build", *common, "-i", "-o", str(root / "program.qzi")],
                 [str(qz), "build", str(root / "program.qzi"), *common, "-o", str(binary)]]
    else:
        steps = [[str(qz), "build", *common, "-o", str(binary)]]
    for step in steps:
        result = subprocess.run(step, cwd=root, text=True, capture_output=True, timeout=300)
        assert result.returncode == 0, result.stdout + result.stderr
    env = dict(os.environ)
    env.pop("QUAZI_TRACE", None)
    env.update(environment or {})
    return subprocess.run([str(binary)], cwd=root, text=True, capture_output=True,
                          timeout=60, env=env)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--qz", type=Path, default=Path("target/debug/qz"))
    qz = parser.parse_args().qz.resolve()
    with tempfile.TemporaryDirectory(prefix="quazi-stack-") as directory:
        root = Path(directory)
        (root / "src").mkdir()
        (root / "quazi.toml").write_text('[package]\nname="stack"\n[build]\nentry="src/main.qz"\n')
        for qzi in (False, True):
            overflow = build(qz, root, RECURSION, qzi)
            assert overflow.returncode == 134, (overflow.returncode, overflow.stderr)
            assert "has overflowed its stack" in overflow.stderr, overflow.stderr
            fault = build(qz, root, NULL_READ, qzi)
            assert fault.returncode == 101, (fault.returncode, fault.stderr)
            assert "== CRASHED ==" in fault.stderr and "overflowed" not in fault.stderr, fault.stderr
            assert fault.stderr.endswith("stack trace\n"), repr(fault.stderr)
            traced = build(qz, root, NULL_READ, qzi, {"QUAZI_TRACE": "2"})
            assert traced.returncode == 101, (traced.returncode, traced.stderr)
            assert " in main\n" in traced.stderr, traced.stderr
            assert "registers:\n" in traced.stderr and "  rip    = 0x" in traced.stderr, traced.stderr
            assert "stack backtrace:" in traced.stderr, traced.stderr
            plain = build(qz, root, NULL_READ, qzi, {"QUAZI_TRACE": "1"})
            assert "registers:" not in plain.stderr and "stack backtrace:" in plain.stderr, plain.stderr
            frames = build(qz, root, NAMED_FRAMES, qzi, {"QUAZI_TRACE": "1"})
            assert frames.returncode == 101, (frames.returncode, frames.stderr)
            names = [line.rsplit(" in ", 1)[1] for line in frames.stderr.splitlines() if " in " in line]
            assert names[-3:] == ["inner", "middle", "main"], frames.stderr
            nested = build(qz, root, NESTED_PANIC, qzi)
            assert nested.returncode == 101, (nested.returncode, nested.stderr)
            assert "panicked while processing a panic: 'inner failure'" in nested.stderr, nested.stderr
    print("PASS stack overflow: recursion reports overflow (134), null read keeps crash report (101) "
          "with named frames and QUAZI_TRACE=2 registers, panic in panic handler aborts (101); "
          "native and QZI")


if __name__ == "__main__":
    main()
