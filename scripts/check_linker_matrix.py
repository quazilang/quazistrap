#!/usr/bin/env python3
"""Target-native linker validation matrix.

Every program is linked three ways and executed where that output runs:

- Linux ELF from the built-in linker, run natively;
- Linux ELF from the external GNU `ld` (when installed), run natively;
- Windows PE32+ from the built-in linker, run under Wine (`--wine`).

Standard output and exit status must be identical across the matrix. The
programs are deterministic examples plus focused cases for arguments, exit
status, panics, heap traffic and threads.
"""

import argparse
import shutil
import subprocess
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# Deterministic examples that need neither input, network nor files.
EXAMPLES = [
    "01-hello-world", "02-struct-methods", "03-enum-pattern-matching", "04-closures",
    "05-generics", "08-dynamic-arrays", "09-modules-and-imports", "10-bitwise-operations",
    "11-conditional-branches", "12-loop-control", "15-boolean-logic", "16-module-visibility",
    "17-constant-expressions", "18-string-formatting", "23-standard-library-tour",
    "25-local-dependency", "27-text-and-math", "28-git-library-dependency",
    "35-multi-slot-arrays", "37-qoroutines",
]

PROGRAMS = {
    # Arguments reach main and the status leaves the process.
    "arguments": (r"""import std.io;

fn main(args: Array[str]) i32 {
    io.println("argc {}", args.len());
    for index : 1..args.len() { io.println("arg {}", args[index]); }
    ret 3;
}
""", ["alpha", "λ🙂"]),
    # A panic ends the process with status 101 after its report on stderr.
    "panicking": (r"""import std.io;

fn main() i32 {
    io.println("before");
    const values = [1, 2, 3];
    var index: usize = 7;
    ret values[index];
}
""", []),
    # Heap traffic through owned strings and growing arrays.
    "heap": (r"""import std.io;

fn main() i32 {
    var total: usize = 0;
    for round : 0..20000 {
        var lengths: Array[usize] = Array.new();
        for i : 0..8 {
            const text = "item {round}-{i}";
            lengths.push(text.len());
        }
        for index : 0..lengths.len() { total = total + lengths[index]; }
    }
    io.println("bytes {total}");
    0
}
""", []),
    # realloc(NULL, n) allocates (HeapReAlloc rejected it on Windows), and
    # buffered file I/O built on it round-trips CRLF lines.
    "buffered": (r"""import std.io;
import std.fs;
import std.core;

fn main() i32 {
    unsafe {
        const block: *u8 = core.realloc(0, 32);
        if (block == 0) { ret 9; }
        core.free(block);
    }
    var writer = fs.BufferedWriter.create("matrix_buffered.txt").unwrap();
    for i : 0..5000 {
        const line = "row {i}\r\n";
        writer.write_str(line.as_str()).unwrap();
    }
    writer.close().unwrap();
    var reader = fs.LineReader.open("matrix_buffered.txt").unwrap();
    var count: usize = 0;
    var seen: usize = 0;
    for (true) {
        const line = reader.read_line().unwrap();
        if (line.is_none()) { break; }
        const text: String = line.unwrap();
        seen = seen + text.bytes_len();
        count = count + 1;
    }
    // Windows cannot delete a file that is still open.
    reader.close().unwrap();
    fs.remove("matrix_buffered.txt").unwrap();
    io.println("lines {} bytes {}", count, seen);
    0
}
""", []),
    # Offsets past 4 GiB (sparse) and reads in small pieces: partial I/O must
    # assemble the same bytes on every target.
    "large_file": (r"""import std.io;
import std.fs;
import std.core;

fn main() i32 {
    const path = "matrix_large.bin";
    var file = fs.File.create(path).unwrap();
    const far: i64 = 5000000123;
    if (file.seek(far, fs.seek_set()).unwrap() != far) { ret 2; }
    file.write_str("tail-marker").unwrap();
    file.close().unwrap();
    const info = fs.metadata(path).unwrap();
    io.println("size {}", info.len());
    var reader = fs.File.open(path).unwrap();
    if (reader.seek(far - 4, fs.seek_set()).unwrap() != far - 4) { ret 3; }
    var buffer: *u8 = 0;
    unsafe { buffer = core.malloc(32); }
    var total: usize = 0;
    // Three bytes at a time until the end of the file.
    for (true) {
        var count: isize = 0;
        unsafe { count = reader.read(buffer + total, 3); }
        if (count < 0) { ret 4; }
        if (count == 0) { break; }
        total = total + (count as usize);
    }
    var zeros: usize = 0;
    var marker: bool = false;
    unsafe {
        for i : 0..4 { if (*(buffer + i) == 0) { zeros = zeros + 1; } }
        *(buffer + total) = 0;
        marker = core.str_from_ptr(buffer + 4) == "tail-marker";
        core.free(buffer);
    }
    reader.close().unwrap();
    fs.remove(path).unwrap();
    io.println("read {} zeros {} marker {}", total, zeros, marker);
    0
}
""", []),
    # OS threads with closures and joins.
    "threads": (r"""import std.io;
import std.thread;

fn work(seed: i64) i64 {
    var total: i64 = 0;
    for i : 0..100000 { total = total + (seed + i as i64) % 7; }
    total
}

fn main() i32 {
    const a = thread.spawn(|| work(1 as i64)).unwrap();
    const b = thread.spawn(|| work(2 as i64)).unwrap();
    io.println("threads {}", a.join().unwrap() + b.join().unwrap());
    0
}
""", []),
}


def run(command, cwd, timeout=120):
    return subprocess.run(command, cwd=cwd, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                          stdin=subprocess.DEVNULL, timeout=timeout)


def build(qz, project, output, extra):
    result = run([str(qz), "build", "--no-incremental", "--no-progress", "--no-color",
                  *extra, "-o", str(output)], project, timeout=600)
    assert result.returncode == 0, (project, extra, result.stderr.decode(errors="replace"))


def normalize(stdout):
    return stdout.decode(errors="replace").replace("\r\n", "\n")


def check(qz, wine, root):
    variants = [("linux-builtin", ["--target", "x86_64-linux", "--linker", "builtin"], None)]
    if shutil.which("ld"):
        variants.append(("linux-ld", ["--target", "x86_64-linux", "--linker", "ld"], None))
    if wine:
        variants.append(("windows-builtin", ["--target", "x86_64-windows"], wine))

    cases = [(name, ROOT / "examples" / name, []) for name in EXAMPLES]
    for name, (source, arguments) in PROGRAMS.items():
        project = root / name
        (project / "src").mkdir(parents=True)
        (project / "quazi.toml").write_text(
            f'[package]\nname = "{name}"\n\n[build]\nentry = "src/main.qz"\n')
        (project / "src/main.qz").write_text(source)
        cases.append((name, project, arguments))

    failures = []
    for name, project, arguments in cases:
        outcomes = {}
        for variant, flags, runner in variants:
            suffix = ".exe" if runner else ""
            output = root / "out" / f"{name}-{variant}{suffix}"
            output.parent.mkdir(exist_ok=True)
            build(qz, project, output, flags)
            command = ([str(runner)] if runner else []) + [str(output), *arguments]
            result = run(command, project)
            outcomes[variant] = (result.returncode, normalize(result.stdout))
        reference = outcomes["linux-builtin"]
        for variant, outcome in outcomes.items():
            if outcome != reference:
                failures.append(f"{name}: {variant} {outcome!r} != linux-builtin {reference!r}")
        if name == "panicking":
            assert reference[0] == 101, reference
        if name == "large_file":
            assert reference == (0, "size 5000000134\nread 15 zeros 4 marker true\n"), reference
    assert not failures, "\n".join(failures)
    print(f"PASS linker matrix: {len(cases)} programs x {[v[0] for v in variants]} "
          "agree on stdout and exit status")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--qz", type=Path, default=Path("target/debug/qz"))
    parser.add_argument("--wine", type=Path, help="Wine executable for the Windows column")
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="quazi-linker-matrix-") as directory:
        check(args.qz.resolve(), args.wine, Path(directory))


if __name__ == "__main__":
    main()
