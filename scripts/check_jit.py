#!/usr/bin/env python3
"""Check experimental `qz run --jit` (Linux x86-64): in-memory execution with
the program's stdout, arguments and exit status, and no executable on disk."""

import argparse
import os
import signal
import subprocess
import tempfile
from pathlib import Path

PROGRAM = r"""import std.io;

fn main(args: Array[str]) i32 {
    io.println("hello from jit");
    io.println("argc {}", args.len());
    var total: i64 = 0;
    for i : 0..1000 { total = total + i; }
    io.println("total {}", total);
    ret 7;
}
"""

FLOOD = r"""import std.io;

fn main() i32 {
    for i : 0..1000000 { io.println("line {}", i); }
    ret 0;
}
"""

PANIC = r"""fn main() i32 {
    const values = [1, 2, 3];
    var index: usize = 5;
    ret values[index];
}
"""


def run(command, cwd, env=None):
    return subprocess.run(command, cwd=cwd, text=True, stdout=subprocess.PIPE,
                          stderr=subprocess.PIPE, timeout=120, env=env)


def check(qz, root):
    common = ["--no-incremental", "--no-progress", "--no-color"]
    (root / "hello.qz").write_text(PROGRAM)
    result = run([str(qz), "run", "--jit", *common, "hello.qz"], root)
    assert result.returncode == 7, (result.returncode, result.stdout, result.stderr)
    assert result.stdout == "hello from jit\nargc 1\ntotal 499500\n", result.stdout
    assert not (root / "hello").exists(), "--jit must not write an executable"

    # A non-UTF-8 environment entry reaches the program as raw bytes.
    environment = os.environb.copy()
    environment[b"QUAZI_JIT_BYTES"] = b"\xff\xfe"
    result = subprocess.run([str(qz), "run", "--jit", *common, "hello.qz"], cwd=root,
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=environment,
                            timeout=120)
    assert result.returncode == 7, (result.returncode, result.stderr)

    # Like a native executable, a closed stdout pipe ends the program by SIGPIPE.
    (root / "flood.qz").write_text(FLOOD)
    reader = subprocess.Popen([str(qz), "run", "--jit", *common, "flood.qz"], cwd=root,
                              stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
    reader.stdout.readline()
    reader.stdout.close()
    assert reader.wait(timeout=120) == -signal.SIGPIPE, reader.returncode

    # A panic inside the JIT-run program ends `qz` with the panic status.
    (root / "panic.qz").write_text(PANIC)
    result = run([str(qz), "run", "--jit", *common, "panic.qz"], root)
    assert result.returncode == 101, (result.returncode, result.stdout, result.stderr)
    assert not (root / "panic").exists()

    # External linkers cannot feed the in-memory loader.
    result = run([str(qz), "run", "--jit", *common, "--linker", "cc", "hello.qz"], root)
    assert result.returncode != 0 and "requires the built-in linker" in result.stderr, result.stderr

    # Without --jit, `qz run` still builds and runs an executable.
    result = run([str(qz), "run", *common, "hello.qz"], root)
    assert result.returncode == 7, (result.returncode, result.stderr)
    assert (root / "hello").exists()
    print("jit: in-memory run, exit status, raw environment, SIGPIPE, panic status, linker guard pass")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--qz", type=Path, default=Path("target/debug/qz"))
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="quazi-jit-") as directory:
        check(args.qz.resolve(), Path(directory))


if __name__ == "__main__":
    main()
