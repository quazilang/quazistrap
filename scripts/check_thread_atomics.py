#!/usr/bin/env python3
"""Check atomics under real OS-thread contention and argument-carrying thread spawn on
Linux, with pthreads (`libc = true`) and with the embedded libc-free clone runtime."""

import argparse
import subprocess
import tempfile
from pathlib import Path

STD_PROGRAM = r"""import std.io;
import std.core;
import std.thread;
import std.thread.{AtomicI64, Mutex};

// Shared block: [0] mutex state pointer, [8] atomic cell pointer, [16] plain counter.
@export("std_sync_worker")
pub unsafe fn worker(shared: *u8) *u8 {
    const lock = Mutex.at(*(shared as **u8));
    const atomic = AtomicI64.at(*((shared + 8) as **u8));
    for i : 0..50000 {
        lock.lock();
        *((shared + 16) as *i64) = *((shared + 16) as *i64) + 1;
        lock.unlock();
        atomic.fetch_add(2);
    }
    ret 0;
}

fn main() i32 {
    const lock = Mutex.new();
    const atomic = AtomicI64.new(0);
    var counted: i64 = 0;
    unsafe {
        const shared: *u8 = core.malloc(24);
        *(shared as **u8) = lock.as_ptr();
        *((shared + 8) as **u8) = atomic.as_ptr();
        *((shared + 16) as *i64) = 0;
        const a = thread.thread_spawn_arg(worker, shared);
        const b = thread.thread_spawn_arg(worker, shared);
        const c = thread.thread_spawn_arg(worker, shared);
        const d = thread.thread_spawn_arg(worker, shared);
        thread.thread_join(a); thread.thread_join(b); thread.thread_join(c); thread.thread_join(d);
        counted = *((shared + 16) as *i64);
        core.free(shared);
    }
    io.println("mutex {} atomic {}", counted, atomic.load());
    if (counted != 200000 || atomic.load() != 400000) { ret 1; }
    0
}
"""

PROGRAM = r"""import std.io;
import std.core;

@repr(C)
type ThreadCallback = fn(*u8) *u8;

@intrinsic("quazi.thread.spawn_arg")
unsafe fn spawn_arg(f: ThreadCallback, arg: *u8) usize;
@intrinsic("quazi.thread.join")
unsafe fn join(handle: usize) void;

@export("mt_atomic_worker")
pub unsafe fn atomic_worker(counter: *u8) *u8 {
    for i : 0..100000 { core.atomic_add(counter as *i64, 1); }
    ret 0;
}

@export("mt_plain_worker")
pub unsafe fn plain_worker(counter: *u8) *u8 {
    for i : 0..100000 { *(counter as *i64) = *(counter as *i64) + 1; }
    ret 0;
}

unsafe fn run(worker: ThreadCallback) i64 {
    const counter: *u8 = core.malloc(8);
    *(counter as *i64) = 0;
    const a = spawn_arg(worker, counter);
    const b = spawn_arg(worker, counter);
    const c = spawn_arg(worker, counter);
    const d = spawn_arg(worker, counter);
    join(a); join(b); join(c); join(d);
    const total: i64 = *(counter as *i64);
    core.free(counter);
    total
}

fn main() i32 {
    var atomic: i64 = 0;
    var plain: i64 = 0;
    unsafe {
        atomic = run(atomic_worker);
        plain = run(plain_worker);
    }
    io.println("atomic {} plain {}", atomic, plain);
    if (atomic != 400000) { ret 1; }
    0
}
"""


CHURN_PROGRAM = r"""import std.io;
import std.core;
import std.fs;
import std.thread;

// Mapped address space of this process in pages (first field of statm).
fn mapped_pages() u64 {
    const result = fs.read_to_string("/proc/self/statm");
    if (result.is_err()) { ret 0; }
    const text: String = result.unwrap();
    const end: usize = text.as_str().find(" ").unwrap();
    const field = text[0:end];
    const parsed: Result[u64, ParseError] = field.parse[u64]();
    parsed.unwrap_or(0)
}

@export("churn_worker")
pub unsafe fn worker(counter: *u8) *u8 {
    core.atomic_add(counter as *i64, 1);
    ret 0;
}

@export("forever_worker")
pub unsafe fn forever(flag: *u8) *u8 {
    core.atomic_add(flag as *i64, 1);
    for (true) { }
    ret 0;
}

fn main() i32 {
    var total: i64 = 0;
    unsafe {
        const counter: *u8 = core.malloc(8);
        *(counter as *i64) = 0;
        // Each thread maps an 8 MiB region; join must unmap it.
        for i : 0..2000 {
            const handle = thread.thread_spawn_arg(worker, counter);
            if (handle == 0) { ret 2; }
            thread.thread_join(handle);
        }
        total = *(counter as *i64);
        const pages = mapped_pages();
        // 2000 leaked regions would be about 4 million pages.
        if (pages == 0 || pages > 65536) { io.println("mapped pages {}", pages); ret 3; }
        // Returning from main ends the process although this thread never stops.
        *(counter as *i64) = 0;
        thread.thread_spawn_arg(forever, counter);
        for (core.atomic_add(counter as *i64, 0) == 0) { }
    }
    io.println("churn {}", total);
    if (total != 2000) { ret 1; }
    0
}
"""


FORMAT_PROGRAM = r"""import std.io;
import std.thread;

// Formats numbers concurrently and parses them back: shared digit scratch
// would hand one thread another thread's digits.
fn mismatches(seed: i64) i64 {
    var wrong: i64 = 0;
    for i : 0..100000 {
        const n: i64 = seed * 1000000 + i as i64;
        const text = "x{n}";
        const parsed: Result[i64, ParseError] = text[1:text.len()].parse[i64]();
        if (parsed.unwrap_or(-1) != n) { wrong = wrong + 1; }
    }
    wrong
}

fn main() i32 {
    const a = thread.spawn(|| mismatches(1 as i64)).unwrap();
    const b = thread.spawn(|| mismatches(2 as i64)).unwrap();
    const c = thread.spawn(|| mismatches(3 as i64)).unwrap();
    const d = thread.spawn(|| mismatches(4 as i64)).unwrap();
    const wrong = a.join().unwrap() + b.join().unwrap() + c.join().unwrap() + d.join().unwrap();
    io.println("format mismatches {wrong}");
    if (wrong != 0) { ret 1; }
    0
}
"""


def invoke(command, root):
    result = subprocess.run(command, cwd=root, text=True, stdout=subprocess.PIPE,
                            stderr=subprocess.STDOUT, timeout=300)
    assert result.returncode == 0, f"{command!r} returned {result.returncode}:\n{result.stdout}"
    return result.stdout


def check(qz, root):
    common = ["--no-incremental", "--no-color", "--no-unicode", "--silent"]
    case = root / "threads"
    (case / "src").mkdir(parents=True)
    (case / "quazi.toml").write_text(
        '[package]\nname="threads"\n[build]\nentry="src/main.qz"\n'
        '[target.x86_64-linux.link]\nlibc = true\n')
    (case / "src/main.qz").write_text(PROGRAM)
    binary = case / "native"
    invoke([str(qz), "build", *common, "-o", str(binary)], case)
    for _ in range(3):
        output = invoke([str(binary)], case)
        assert output.startswith("atomic 400000 "), output
    coff = case / "program.obj"
    invoke([str(qz), "build", *common, "-c", "--target", "x86_64-windows", "-o", str(coff)], case)
    assert coff.read_bytes()[:2] == b"\x64\x86"
    print("PASS threads: 4 OS threads x 100000 atomic increments = 400000 (3 runs); Windows COFF emits")
    case = root / "std_sync"
    (case / "src").mkdir(parents=True)
    (case / "quazi.toml").write_text(
        '[package]\nname="std_sync"\n[build]\nentry="src/main.qz"\n'
        '[target.x86_64-linux.link]\nlibc = true\n')
    (case / "src/main.qz").write_text(STD_PROGRAM)
    binary = case / "native"
    invoke([str(qz), "build", *common, "-o", str(binary)], case)
    assert b"pthread_create" in binary.read_bytes(), "libc builds must keep pthreads"
    for _ in range(3):
        output = invoke([str(binary)], case)
        assert output.startswith("mutex 200000 atomic 400000"), output
    print("PASS std.thread: Mutex-protected counter 200000 and AtomicI64 400000 across 4 OS threads (3 runs)")

    # Without libc, Linux threads use the embedded clone/futex runtime: the
    # same std program, the built-in linker (static, no dynamic section) and
    # `qz run --jit`.
    case = root / "std_sync_nolibc"
    (case / "src").mkdir(parents=True)
    (case / "quazi.toml").write_text('[package]\nname="std_sync_nolibc"\n[build]\nentry="src/main.qz"\n')
    (case / "src/main.qz").write_text(STD_PROGRAM)
    binary = case / "native"
    invoke([str(qz), "build", *common, "-o", str(binary)], case)
    assert b"pthread" not in binary.read_bytes(), "libc-free build must not reference pthreads"
    for _ in range(3):
        output = invoke([str(binary)], case)
        assert output.startswith("mutex 200000 atomic 400000"), output
    output = invoke([str(qz), "run", "--jit", *common], case)
    assert "mutex 200000 atomic 400000" in output, output
    case = root / "churn"
    (case / "src").mkdir(parents=True)
    (case / "quazi.toml").write_text('[package]\nname="churn"\n[build]\nentry="src/main.qz"\n')
    (case / "src/main.qz").write_text(CHURN_PROGRAM)
    binary = case / "native"
    invoke([str(qz), "build", *common, "-o", str(binary)], case)
    output = invoke([str(binary)], case)
    assert output.startswith("churn 2000"), output
    print("PASS libc-free threads: std Mutex/AtomicI64 native and --jit, 2000 spawn/join without leaking stacks, main exit ends running threads")
    case = root / "format"
    (case / "src").mkdir(parents=True)
    (case / "quazi.toml").write_text('[package]\nname="format"\n[build]\nentry="src/main.qz"\n')
    (case / "src/main.qz").write_text(FORMAT_PROGRAM)
    binary = case / "native"
    invoke([str(qz), "build", *common, "-o", str(binary)], case)
    for _ in range(3):
        output = invoke([str(binary)], case)
        assert output.startswith("format mismatches 0"), output
    print("PASS thread formatting: 4 threads x 100000 interpolated numbers parse back exactly (3 runs)")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--qz", type=Path, default=Path("target/debug/qz"))
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="quazi-threads-") as directory:
        check(args.qz.resolve(), Path(directory))


if __name__ == "__main__":
    main()
