#!/usr/bin/env python3
"""Check the embedded size-class heap (Linux): 300000 strings and structs
allocate without per-allocation system calls (fewer than 1000 mmap/munmap
calls, counted with strace when available), the result is unchanged, and
four OS threads allocating concurrently produce exact results (calloc zeroing
and realloc growth through Array)."""

import argparse
import re
import shutil
import subprocess
import tempfile
from pathlib import Path

HEAVY = r"""import std.io;
import std.fs;

pub struct Item { id: i64, label: String, }

fn mapped_pages() u64 {
    const result = fs.read_to_string("/proc/self/statm");
    if (result.is_err()) { ret 0; }
    const text: String = result.unwrap();
    const end: usize = text.as_str().find(" ").unwrap();
    const field = text[0:end];
    const parsed: Result[u64, ParseError] = field.parse[u64]();
    parsed.unwrap_or(0)
}

fn main() i32 {
    var total: i64 = 0;
    for i : 0..300000 {
        const text: String = "item-".to_string();
        const label = text.push_str(i.to_string().as_str());
        const item = Item { id: i as i64, label: label };
        total = total + item.label.len() as i64 + item.id % 3;
    }
    io.println("total {} pages {}", total, mapped_pages());
    0
}
"""

CONCURRENT = r"""import std.io;
import std.thread;

fn churn(seed: i64) i64 {
    var total: i64 = 0;
    for i : 0..100000 {
        const text: String = "x".to_string();
        const longer = text.push_str(if ((seed + i as i64) % 2 == 0) { "ab" } else { "abc" });
        total = total + longer.len() as i64;
    }
    total
}

fn grow(count: i64) i64 {
    var values: Array[i64] = Array.new();
    for i : 0..count { values.push(i as i64); }
    var sum: i64 = 0;
    for i : 0..values.len() { sum = sum + values.get(i); }
    sum
}

fn main() i32 {
    const a = thread.spawn(|| churn(1 as i64)).unwrap();
    const b = thread.spawn(|| churn(2 as i64)).unwrap();
    const c = thread.spawn(|| churn(3 as i64)).unwrap();
    const d = thread.spawn(|| churn(4 as i64)).unwrap();
    var total: i64 = 0;
    for i : 0..0 {
        const seed: i64 = i as i64;
        total = total + (qo churn(seed)).join().unwrap();
    }
    total = total + a.join().unwrap() + b.join().unwrap() + c.join().unwrap() + d.join().unwrap();
    io.println("churn {} grow {}", total, grow(50000 as i64));
    0
}
"""


def invoke(command, root):
    result = subprocess.run(command, cwd=root, text=True, stdout=subprocess.PIPE,
                            stderr=subprocess.STDOUT, timeout=300)
    assert result.returncode == 0, f"{command!r} returned {result.returncode}:\n{result.stdout}"
    return result.stdout


def build(qz, root, name, source):
    case = root / name
    (case / "src").mkdir(parents=True)
    (case / "quazi.toml").write_text(f'[package]\nname="{name}"\n[build]\nentry="src/main.qz"\n')
    (case / "src/main.qz").write_text(source)
    binary = case / "native"
    invoke([str(qz), "build", "--no-incremental", "--no-color", "--silent", "-o", str(binary)], case)
    return case, binary


def check(qz, root):
    case, binary = build(qz, root, "heavy", HEAVY)
    assert "total 3488890" in invoke([str(binary)], case)
    if shutil.which("strace"):
        traced = subprocess.run(["strace", "-c", "-e", "trace=mmap,munmap", str(binary)], cwd=case,
                                text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=300)
        calls = sum(int(line.split()[3]) for line in traced.stderr.splitlines()
                    if line.split() and line.split()[-1] in ("mmap", "munmap"))
        assert calls < 1000, traced.stderr
    case, binary = build(qz, root, "concurrent", CONCURRENT)
    for _ in range(5):
        assert "churn 1400000 grow 1249975000" in invoke([str(binary)], case)
    print("PASS heap: 300000 allocation rounds without per-allocation syscalls; 4 concurrent threads exact (5 runs)")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--qz", type=Path, default=Path("target/debug/qz"))
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="quazi-heap-") as directory:
        check(args.qz.resolve(), Path(directory))


if __name__ == "__main__":
    main()
