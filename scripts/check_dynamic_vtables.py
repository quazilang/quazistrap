#!/usr/bin/env python3
"""Exercise portable dynamic dispatch, namespace aliases, and incremental units."""

import argparse
import hashlib
import re
import shutil
import subprocess
import tempfile
from pathlib import Path


IMPLEMENTATION = """pub struct Counter { padding: i64, value: i32 }
pub trait Readable { fn read(self: &Self) i32; }
impl Readable for Counter {
    @inline fn read(self: &Counter) i32 { ret self.value; }
}
"""

CREATOR = """import counter.Counter;
import counter.Readable;
fn main() i32 {
    var value: dyn Readable = Counter { padding: 99, value: 42 };
    ret value.read() - 42;
}
"""


def project(root, name, source, *, unmangled=False, library=False):
    (root / "src").mkdir(parents=True)
    manifest = (
        f'[package]\nname = "{name}"\nstd = false\ncrash_handler = false\n'
    )
    if unmangled:
        manifest += "mangling = false\n"
    if library:
        manifest += '[lib]\npath = "src/lib.qz"\n'
    (root / "quazi.toml").write_text(manifest)
    (root / "src" / ("lib.qz" if library else "main.qz")).write_text(source)


def run(qz, root, *args, expected=0):
    command = [str(qz), *map(str, args), "--no-color", "--no-unicode"]
    result = subprocess.run(
        command, cwd=root, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
        timeout=60
    )
    if result.returncode != expected:
        raise AssertionError(
            f"Command {command!r} in {root} returned {result.returncode}, "
            f"expected {expected}:\n{result.stdout}"
        )
    return result.stdout


def portable_outputs(qz, root, name):
    artifact = root / f"{name}.qzi"
    run(qz, root, "run", "--no-incremental", "--no-progress")
    run(qz, root, "build", "-i", "-o", artifact, "--no-incremental")
    assert artifact.read_bytes()[:5] == b"\x00QZI\x0d", "expected QZI v13"
    run(qz, root, "run", artifact, "--no-incremental", "--no-progress")
    coff = root / f"{name}.obj"
    run(
        qz, root, "build", artifact, "-c", "--target", "x86_64-windows",
        "-o", coff, "--no-incremental",
    )
    assert coff.read_bytes()[:2] == b"\x64\x86", "expected x86-64 COFF"


def imported_dispatch(qz, root):
    project(root, "imported_dispatch", CREATOR, unmangled=True)
    (root / "src/counter.qz").write_text(IMPLEMENTATION)
    portable_outputs(qz, root, "imported")
    print("PASS imported trait/type: native, QZI-only, Windows COFF, mangling=false")


def incremental_dispatch(qz, root):
    project(root, "incremental_dispatch", CREATOR)
    implementation = root / "src/counter.qz"
    implementation.write_text(IMPLEMENTATION)
    creator_hash = hashlib.sha256((root / "src/main.qz").read_bytes()).digest()
    cold = run(qz, root, "run")
    assert "cache lookup  miss" in cold, cold
    exact = run(qz, root, "run")
    assert "cache lookup  hit" in exact, exact
    implementation.write_text(IMPLEMENTATION.replace("ret self.value;", "ret self.value + 1;"))
    partial = run(qz, root, "run", expected=1)
    assert "cache lookup  partial" in partial, partial
    stats = re.search(r"(\d+) restored\s*[|·]\s*(\d+) compiled", partial)
    assert stats and all(int(value) > 0 for value in stats.groups()), partial
    assert hashlib.sha256((root / "src/main.qz").read_bytes()).digest() == creator_hash
    # The newly linked cache must also preserve the changed implementation.
    updated = run(qz, root, "run", expected=1)
    assert "cache lookup  hit" in updated, updated
    print(f"PASS incremental: cold, exact, partial ({stats.group(0)}), updated exact")


def aliased_library_dispatch(qz, root):
    library = root / "library"
    project(library, "original", """struct PrivateCounter { value: i32 }
trait PrivateReadable { fn read(self: &Self) i32; }
impl PrivateReadable for PrivateCounter {
    @inline fn read(self: &PrivateCounter) i32 { ret self.value + 1; }
}
pub fn answer(seed: i32) i32 {
    var value: dyn PrivateReadable = PrivateCounter { value: seed };
    ret value.read();
}
""", library=True)
    artifact = root / "original.qzi"
    run(qz, library, "build", "--lib", "-i", "-o", artifact, "--no-incremental")
    shutil.rmtree(library / "src")
    consumer = root / "consumer"
    project(consumer, "consumer", """import renamed.answer;
fn main() i32 { ret answer(41) - 42; }
""")
    with (consumer / "quazi.toml").open("a") as manifest:
        manifest.write('[dependencies]\nrenamed = { path = "../original.qzi" }\n')
    portable_outputs(qz, consumer, "consumer")
    print("PASS aliased QZI library: scalar API, private dyn, defining source removed")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("qz", type=Path, help="compiler executable")
    qz = parser.parse_args().qz.resolve(strict=True)
    with tempfile.TemporaryDirectory(prefix="quazi-dynamic-vtables-") as temporary:
        root = Path(temporary)
        imported_dispatch(qz, root / "imported")
        incremental_dispatch(qz, root / "incremental")
        aliased_library_dispatch(qz, root / "aliased")


if __name__ == "__main__":
    main()
