#!/usr/bin/env python3
"""Check `qo f(args)` (D-019): qoroutines on the prelude M:N scheduler run
natively, from source-removed QZI and under `qz run --jit`; Windows COFF and
NetBSD ELF objects emit; programs without `qo` do not contain the scheduler."""

import argparse
import subprocess
import tempfile
from pathlib import Path

PROGRAM = r"""import std.io;

fn square(n: i64) i64 { n * n }

fn is_even(n: i64) bool { n % 2 == 0 }

fn main() i32 {
    var total: i64 = 0;
    var evens: i64 = 0;
    const first = qo square(3 as i64);
    for i : 0..300 {
        const n: i64 = i as i64;
        const handle: Qoroutine[i64] = qo square(n);
        const even = qo is_even(n);
        total = total + handle.join();
        if (even.join()) { evens = evens + 1; }
    }
    total = total + first.join();
    io.println("total {} evens {}", total, evens);
    if (total != 8955059 || evens != 150) { ret 1; }
    0
}
"""

EXPECTED = "total 8955059 evens 150"


def invoke(command, root, ok=True):
    result = subprocess.run(command, cwd=root, text=True, stdout=subprocess.PIPE,
                            stderr=subprocess.STDOUT, timeout=300)
    if ok:
        assert result.returncode == 0, f"{command!r} returned {result.returncode}:\n{result.stdout}"
    return result


def check(qz, root):
    common = ["--no-incremental", "--no-color", "--no-unicode", "--silent"]
    case = root / "qoroutines"
    (case / "src").mkdir(parents=True)
    (case / "quazi.toml").write_text('[package]\nname="qoroutines"\n[build]\nentry="src/main.qz"\n')
    (case / "src/main.qz").write_text(PROGRAM)
    binary = case / "native"
    invoke([str(qz), "build", *common, "-o", str(binary)], case)
    for _ in range(5):
        assert EXPECTED in invoke([str(binary)], case).stdout
    assert EXPECTED in invoke([str(qz), "run", "--jit", *common], case).stdout
    coff = case / "program.obj"
    invoke([str(qz), "build", *common, "-c", "--target", "x86_64-windows", "-o", str(coff)], case)
    assert coff.read_bytes()[:2] == b"\x64\x86"
    netbsd = case / "program.o"
    invoke([str(qz), "build", *common, "-c", "--target", "x86_64-netbsd", "-o", str(netbsd)], case)
    assert netbsd.read_bytes()[:4] == b"\x7fELF"
    artifact = case / "program.qzi"
    invoke([str(qz), "build", *common, "-i", "-o", str(artifact)], case)
    assert b"__qo_worker" in artifact.read_bytes()
    (case / "src/main.qz").unlink()
    portable = case / "portable"
    invoke([str(qz), "build", str(artifact), *common, "-o", str(portable)], root)
    assert EXPECTED in invoke([str(portable)], case).stdout

    plain = root / "plain"
    (plain / "src").mkdir(parents=True)
    (plain / "quazi.toml").write_text('[package]\nname="plain"\n[build]\nentry="src/main.qz"\n')
    (plain / "src/main.qz").write_text('import std.io;\n\nfn main() i32 {\n    io.println("hi");\n    0\n}\n')
    artifact = plain / "program.qzi"
    invoke([str(qz), "build", *common, "-i", "-o", str(artifact)], plain)
    assert b"__qo_" not in artifact.read_bytes(), "the scheduler must be dropped when unused"

    bad = root / "bad"
    (bad / "src").mkdir(parents=True)
    (bad / "quazi.toml").write_text('[package]\nname="bad"\n[build]\nentry="src/main.qz"\n')
    (bad / "src/main.qz").write_text("fn main() i32 {\n    const x = qo 5;\n    0\n}\n")
    result = invoke([str(qz), "check"], bad, ok=False)
    assert result.returncode != 0 and "`qo` must be followed by a function call" in result.stdout, result.stdout
    print("PASS qoroutines: qo on 4 workers (5 runs), --jit, source-removed QZI, COFF/NetBSD emit, unused scheduler dropped, non-call rejected")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--qz", type=Path, default=Path("target/debug/qz"))
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="quazi-qo-") as directory:
        check(args.qz.resolve(), Path(directory))


if __name__ == "__main__":
    main()
