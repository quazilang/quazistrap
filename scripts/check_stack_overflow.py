#!/usr/bin/env python3
"""Check that unbounded recursion reports a stack overflow (Linux crash
handler on an alternate signal stack, Windows vectored handler; exit status
134) instead of dying with a bare SIGSEGV, and that other faults keep the generic crash report
(exit status 101). A custom panic handler runs once, for fatal panics only.
Native and QZI; with `--wine`, also Windows executables under Wine."""

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
HANDLER_LIFECYCLE = """import std.core;
import std.io;

@panic_handler
fn handler(info: PanicInfo) ! {
    io.errln("handled: {} at {}:{}", info.message(), info.file(), info.line());
    core.exit(42);
}

fn work(n: i64) i64 {
    if (n > 1) { panic("task failed {}", n); }
    ret n;
}

fn main() i32 {
    const task = qo work(5);
    if (task.join().is_ok()) { ret 1; }
    io.println("task panic reached join");
    panic("main failed");
}
"""
NULL_READ = """fn main() i32 {
    var p: *i64 = 0;
    unsafe { ret (*p) as i32; }
}
"""
# Overflow on a qoroutine stack hits the slot's guard page and on a
# std.thread thread its stack guard; both report from the thread's
# alternate signal stack. (Windows qoroutine slots have a guard page too, but
# without an alternate stack the overflow is not reported there.)
QO_RECURSION = """fn deep(n: i64) i64 { ret deep(n + 1) + 1; }
fn main() i32 { ret (qo deep(0)).join().unwrap() as i32; }
"""
THREAD_RECURSION = """import std.thread;
fn deep(n: i64) i64 { ret deep(n + 1) + 1; }
fn main() i32 { ret thread.spawn(|| deep(0)).unwrap().join().unwrap() as i32; }
"""
# A fault off the main thread (a qoroutine worker, a std.thread thread) gets
# the same report; on Windows the vectored handler must catch it because
# dispatch cannot walk Quazi frames (no unwind tables).
QO_NULL_READ = """fn crash(n: i64) i64 {
    var p: *i64 = 0;
    unsafe { ret *p + n; }
}
fn main() i32 { ret (qo crash(1)).join().unwrap() as i32; }
"""
THREAD_NULL_READ = """import std.thread;
fn crash(n: i64) i64 {
    var p: *i64 = 0;
    unsafe { ret *p + n; }
}
fn main() i32 { ret thread.spawn(|| crash(1)).unwrap().join().unwrap() as i32; }
"""
NAMED_FRAMES = """fn inner(v: i32) i32 { if v > 2 { panic("bad value {}", v); } ret v; }
fn middle(v: i32) i32 { ret inner(v + 1) * 2; }
fn main() i32 { ret middle(5); }
"""


def build(qz, root, source, qzi, environment=None, wine=None):
    """Build and run `source`: natively, through a QZI, or (with `wine`) as a
    Windows executable under Wine."""
    (root / "src/main.qz").write_text(source)
    common = ["--no-incremental", "--no-color", "--no-unicode", "--silent"]
    binary = root / "program"
    if wine:
        binary = root / "program.exe"
        steps = [[str(qz), "build", *common, "--target", "x86_64-windows", "-o", str(binary)]]
    elif qzi:
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
    command = [str(binary)]
    if wine:
        env.update(WINEDEBUG="-all", WINEPREFIX=str(root / "wineprefix"))
        command = [str(wine), str(binary)]
    return subprocess.run(command, cwd=root, text=True, capture_output=True,
                          timeout=300, env=env)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--qz", type=Path, default=Path("target/debug/qz"))
    parser.add_argument("--wine", type=Path, help="Wine executable for the Windows column")
    args = parser.parse_args()
    qz = args.qz.resolve()
    variants = [(False, None), (True, None)]
    if args.wine:
        variants.append((False, args.wine))
    with tempfile.TemporaryDirectory(prefix="quazi-stack-") as directory:
        root = Path(directory)
        (root / "src").mkdir()
        (root / "quazi.toml").write_text('[package]\nname="stack"\n[build]\nentry="src/main.qz"\n')
        for qzi, wine in variants:
            overflow = build(qz, root, RECURSION, qzi, wine=wine)
            assert overflow.returncode == 134, (overflow.returncode, overflow.stderr)
            assert "has overflowed its stack" in overflow.stderr, overflow.stderr
            for source in (THREAD_RECURSION,) if wine else (QO_RECURSION, THREAD_RECURSION):
                worker = build(qz, root, source, qzi, wine=wine)
                assert worker.returncode == 134, (worker.returncode, worker.stderr)
                assert "has overflowed its stack" in worker.stderr, worker.stderr
            fault = build(qz, root, NULL_READ, qzi, wine=wine)
            assert fault.returncode == 101, (fault.returncode, fault.stderr)
            assert "== CRASHED ==" in fault.stderr and "overflowed" not in fault.stderr, fault.stderr
            assert fault.stderr.endswith("stack trace\n"), repr(fault.stderr)
            traced = build(qz, root, NULL_READ, qzi, {"QUAZI_TRACE": "2"}, wine=wine)
            assert traced.returncode == 101, (traced.returncode, traced.stderr)
            assert " in main\n" in traced.stderr, traced.stderr
            assert "registers:\n" in traced.stderr and "  rip    = 0x" in traced.stderr, traced.stderr
            assert "stack backtrace:" in traced.stderr, traced.stderr
            plain = build(qz, root, NULL_READ, qzi, {"QUAZI_TRACE": "1"}, wine=wine)
            assert "registers:" not in plain.stderr and "stack backtrace:" in plain.stderr, plain.stderr
            for source in (QO_NULL_READ, THREAD_NULL_READ):
                worker = build(qz, root, source, qzi, wine=wine)
                assert worker.returncode == 101, (worker.returncode, worker.stderr)
                assert worker.stderr.startswith("== CRASHED ==\n"), worker.stderr
                assert worker.stderr.count("== CRASHED ==") == 1, worker.stderr
                assert " in crash\n" in worker.stderr, worker.stderr
            frames = build(qz, root, NAMED_FRAMES, qzi, {"QUAZI_TRACE": "1"}, wine=wine)
            assert frames.returncode == 101, (frames.returncode, frames.stderr)
            names = [line.rsplit(" in ", 1)[1] for line in frames.stderr.splitlines() if " in " in line]
            assert names[-3:] == ["inner", "middle", "main"], frames.stderr
            nested = build(qz, root, NESTED_PANIC, qzi, wine=wine)
            assert nested.returncode == 101, (nested.returncode, nested.stderr)
            assert "panicked while processing a panic: 'inner failure'" in nested.stderr, nested.stderr
            # A custom handler runs once, for the fatal panic only: a
            # qoroutine panic still reaches its join (D-019, В28).
            lifecycle = build(qz, root, HANDLER_LIFECYCLE, qzi, wine=wine)
            assert lifecycle.returncode == 42, (lifecycle.returncode, lifecycle.stderr)
            assert lifecycle.stdout == "task panic reached join\n", lifecycle.stdout
            assert "qoroutine panicked at 'task failed 5'" in lifecycle.stderr, lifecycle.stderr
            assert lifecycle.stderr.count("handled: ") == 1, lifecycle.stderr
            assert "handled: main failed at src/main.qz:19" in lifecycle.stderr, lifecycle.stderr
    print("PASS stack overflow: recursion reports overflow (134, also in a qoroutine and a thread), null read keeps crash report (101) "
          "with named frames and QUAZI_TRACE=2 registers (also from qoroutine and thread "
          "faults), panic in panic handler aborts (101), "
          "custom handler runs once for the fatal panic while a qoroutine panic reaches join; "
          "native and QZI" + (", Windows under Wine" if args.wine else ""))


if __name__ == "__main__":
    main()
