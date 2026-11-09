#!/usr/bin/env python3
"""Check quazi.context.switch (two stacks hand control back and forth) and
quazi.runtime.state (one zeroed process-wide block) natively, from QZI and as COFF."""

import argparse
import subprocess
import tempfile
from pathlib import Path

PROGRAM = r"""import std.io;
import std.core;

@intrinsic("quazi.context.switch")
unsafe fn context_switch(save: *u8, restore: *u8) void;

@repr(C)
type Entry = fn(*u8) void;

// Shared block: [0..272) main context, [272..544) task context, [544] counter.
@export("qroutine_test_entry")
pub unsafe fn task_entry(shared: *u8) void {
    unsafe {
        for i : 0..3 {
            *((shared + 544) as *i64) = *((shared + 544) as *i64) + 10;
            context_switch(shared + 272, shared);
        }
        *((shared + 552) as *i64) = 1;
        context_switch(shared + 272, shared);
    }
}

fn main() i32 {
    var shared: *u8 = 0;
    var stack: *u8 = 0;
    var observed: i64 = 0;
    unsafe {
        shared = core.malloc(560);
        core.memset(shared, 0, 560);
        stack = core.malloc(65536);
        const entry: Entry = task_entry;
        *((shared + 272 + 48) as *Entry) = entry;
        *((shared + 272 + 56) as **u8) = stack + 65464;
        *((shared + 272 + 72) as **u8) = shared;
        *((shared + 272 + 80) as *u32) = 8064;
        *((shared + 272 + 84) as *u16) = 895;
        *((shared + 272 + 88) as **u8) = stack + 65536;
        *((shared + 272 + 96) as **u8) = stack;
        for round : 0..4 {
            if (*((shared + 552) as *i64) == 1) { break; }
            context_switch(shared, shared + 272);
            observed = observed + *((shared + 544) as *i64);
        }
    }
    io.println("observed {} done {}", observed, 1);
    if (observed != 10 + 20 + 30 + 30) { ret 1; }
    0
}
"""


STATE_PROGRAM = r"""import std.io;

@intrinsic("quazi.runtime.state")
unsafe fn runtime_state() *u8;

unsafe fn bump() i64 {
    const state = runtime_state();
    *((state + 56) as *i64) = *((state + 56) as *i64) + 1;
    *((state + 56) as *i64)
}

fn main() i32 {
    var zero: i64 = 1;
    var count: i64 = 0;
    unsafe {
        const state = runtime_state();
        zero = 0;
        for offset : 0..8 { zero = zero + *((state + offset * 8) as *i64); }
        bump();
        bump();
        count = bump();
    }
    io.println("zero {} count {}", zero, count);
    if (zero != 0 || count != 3) { ret 1; }
    0
}
"""


def invoke(command, root):
    result = subprocess.run(command, cwd=root, text=True, stdout=subprocess.PIPE,
                            stderr=subprocess.STDOUT, timeout=120)
    assert result.returncode == 0, f"{command!r} returned {result.returncode}:\n{result.stdout}"
    return result.stdout


def check(qz, root):
    common = ["--no-incremental", "--no-color", "--no-unicode", "--silent"]
    case = root / "switch"
    (case / "src").mkdir(parents=True)
    (case / "quazi.toml").write_text('[package]\nname="context_switch"\n[build]\nentry="src/main.qz"\n')
    (case / "src/main.qz").write_text(PROGRAM)
    binary = case / "native"
    invoke([str(qz), "build", *common, "-o", str(binary)], case)
    assert "observed 90" in invoke([str(binary)], case)
    artifact = case / "program.qzi"
    invoke([str(qz), "build", *common, "-i", "-o", str(artifact)], case)
    (case / "src/main.qz").unlink()
    portable = case / "portable"
    invoke([str(qz), "build", str(artifact), *common, "-o", str(portable)], root)
    assert "observed 90" in invoke([str(portable)], case)
    coff = case / "program.obj"
    invoke([str(qz), "build", str(artifact), *common, "-c", "--target", "x86_64-windows", "-o", str(coff)], root)
    assert coff.read_bytes()[:2] == b"\x64\x86"
    print("PASS context switch: 4 round trips on a separate stack, native/source-removed QZI; Windows COFF emits")

    case = root / "state"
    (case / "src").mkdir(parents=True)
    (case / "quazi.toml").write_text('[package]\nname="runtime_state"\n[build]\nentry="src/main.qz"\n')
    (case / "src/main.qz").write_text(STATE_PROGRAM)
    binary = case / "native"
    invoke([str(qz), "build", *common, "-o", str(binary)], case)
    assert "zero 0 count 3" in invoke([str(binary)], case)
    artifact = case / "program.qzi"
    invoke([str(qz), "build", *common, "-i", "-o", str(artifact)], case)
    (case / "src/main.qz").unlink()
    portable = case / "portable"
    invoke([str(qz), "build", str(artifact), *common, "-o", str(portable)], root)
    assert "zero 0 count 3" in invoke([str(portable)], case)
    coff = case / "program.obj"
    invoke([str(qz), "build", str(artifact), *common, "-c", "--target", "x86_64-windows", "-o", str(coff)], root)
    assert coff.read_bytes()[:2] == b"\x64\x86"
    print("PASS runtime state: one shared zeroed block across calls, native/source-removed QZI; Windows COFF emits")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--qz", type=Path, default=Path("target/debug/qz"))
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="quazi-context-") as directory:
        check(args.qz.resolve(), Path(directory))


if __name__ == "__main__":
    main()
