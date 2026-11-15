#!/usr/bin/env python3
"""Check `for` over prelude Array[T]: `for value : array` and
`for index, value : array` (index first), copyable elements read in place,
owning elements (String) moved out one by one with `continue`, `break` and
moves into another array, borrowed arrays through a reference parameter,
and `for index, value` over a has_next/next iterator. Peak memory must not
grow with the iteration count (no leak, no double free). Native and QZI."""

import argparse
import resource
import subprocess
import tempfile
from pathlib import Path

PROGRAM = """import std.io.println;
struct Countdown { left: i32 }
impl Countdown {
    fn has_next(self: &Countdown) bool { ret self.left > 0; }
    fn next(self: &Countdown!) Option[i32] { self.left = self.left - 1; Some(self.left) }
}
fn sum(values: &Array[i32]) i32 {
    var total: i32 = 0;
    for value : values { total = total + value; }
    total
}
fn round(n: usize) usize {
    var names: Array[String] = Array.new();
    var i: usize = 0;
    for (i < 8) { names.push(String.from("abcdefghijklmnopqrstuvwxyz0123456789")); i++; }
    var kept: Array[String] = Array.new();
    var total: usize = 0;
    for index, name : names {
        if (index == 1) { continue; }
        if (index == 6 && n % 2 == 0) { break; }
        if (index % 3 == 0) { kept.push(name); continue; }
        total = total + name.len();
    }
    ret total + kept.len();
}
fn main() i32 {
    var values: Array[i32] = Array.new();
    values.push(10);
    values.push(20);
    println("{}", sum(&values));
    for index, value : values { println("{} {}", index, value); }
    const fixed = [7, 8];
    for index, value : fixed { println("{} {}", index, value); }
    for index, value : Countdown { left: 2 } { println("{} {}", index, value); }
    var n: usize = 0;
    var checksum: usize = 0;
    for (n < ROUNDS) { checksum = checksum + round(n); n++; }
    println("{}", checksum / ROUNDS);
    0
}
"""
EXPECTED = "30\n0 10\n1 20\n0 7\n1 8\n0 1\n1 0\n"


def run(command, root):
    result = subprocess.run(command, cwd=root, text=True, capture_output=True, timeout=300)
    assert result.returncode == 0, f"{command!r}: {result.stdout}{result.stderr}"
    return result.stdout


def build(qz, root, rounds, qzi):
    (root / "src/main.qz").write_text(PROGRAM.replace("ROUNDS", str(rounds)))
    common = ["--no-incremental", "--no-color", "--no-unicode", "--silent"]
    binary = root / f"program-{rounds}-{'qzi' if qzi else 'native'}"
    if qzi:
        run([str(qz), "build", *common, "-i", "-o", str(root / "program.qzi")], root)
        run([str(qz), "build", str(root / "program.qzi"), *common, "-o", str(binary)], root)
    else:
        run([str(qz), "build", *common, "-o", str(binary)], root)
    return binary


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--qz", type=Path, default=Path("target/debug/qz"))
    args = parser.parse_args()
    qz = args.qz.resolve()
    with tempfile.TemporaryDirectory(prefix="quazi-array-loops-") as directory:
        root = Path(directory)
        (root / "src").mkdir()
        (root / "quazi.toml").write_text('[package]\nname="array_loops"\n[build]\nentry="src/main.qz"\n')
        for qzi in (False, True):
            small = build(qz, root, 1000, qzi)
            output = run([str(small)], root)
            assert output.startswith(EXPECTED), output
            # Separate processes: compare their own peaks.
            peaks = []
            for rounds in (20000, 200000):
                binary = build(qz, root, rounds, qzi)
                probe = subprocess.run(
                    ["python3", "-c",
                     "import resource,subprocess,sys;"
                     "r=subprocess.run([sys.argv[1]],capture_output=True);"
                     "print(r.returncode, resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss)",
                     str(binary)],
                    text=True, capture_output=True, timeout=300)
                code, peak = probe.stdout.split()
                assert code == "0", probe.stdout
                peaks.append(int(peak))
            assert peaks[1] < peaks[0] + 2048, f"memory grows with iterations: {peaks}"
    print("PASS array loops: value/index-value over Array, arrays and iterators; owning elements moved without leaks; native and QZI")


if __name__ == "__main__":
    main()
