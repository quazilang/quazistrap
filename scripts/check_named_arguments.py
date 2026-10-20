#!/usr/bin/env python3
"""Check declaration-slot argument ordering through native and portable outputs."""

import argparse
import subprocess
import shutil
import tempfile
from pathlib import Path


SOURCE = """import helpers;
import helpers.digits as imported;
fn digits(second: i32, first: i32) i32 { ret second; }
struct Shared { value: i32 }
impl Shared {
    fn digits(self: &Shared, first: i32, second: i32) i32 {
        ret first * 10 + second;
    }
}
struct Legacy { value: i32 }
impl Legacy {
    fn digits(receiver: Legacy, first: i32, second: i32) i32 {
        ret first * 10 + second;
    }
}
struct Exclusive { value: i32 }
impl Exclusive {
    fn digits(self: &Exclusive!, first: i32, second: i32) i32 {
        self.value = first * 10 + second;
        ret self.value;
    }
}
struct Consumed { value: i32 }
impl Consumed {
    fn digits(self: Consumed, first: i32, second: i32) i32 {
        ret first * 10 + second;
    }
}
struct Other { value: i32 }
impl Other {
    fn digits(self: &Other, second: i32, first: i32) i32 {
        ret first * 10 + second;
    }
}
struct Factory { value: i32 }
impl Factory {
    fn digits(first: i32, second: i32) i32 { ret first * 10 + second; }
}
struct Generic[T] { value: T }
impl Generic[T] {
    fn digits(self: &Generic[T], first: i32, second: i32) i32 {
        ret first * 10 + second;
    }
    fn create(first: T, second: T) Generic[T] { ret Generic { value: first }; }
}
fn forward[T](value: T) i32 {
    var instance: Generic[T] = Generic { value: value };
    ret instance.digits(second = 2, first = 1);
}
fn formatted(first: str, ...rest: str) i32 { ret first.len() as i32; }
fn main() i32 {
    var shared = Shared { value: 0 };
    if shared.digits(1, 2) != 12 { ret 1; }
    if shared.digits(second = 2, first = 1) != 12 { ret 2; }
    if shared.digits(first = 1, second = 2) != 12 { ret 3; }
    var legacy = Legacy { value: 0 };
    if legacy.digits(1, 2) != 12 { ret 4; }
    if legacy.digits(second = 2, first = 1) != 12 { ret 5; }
    var exclusive = Exclusive { value: 0 };
    if exclusive.digits(second = 2, first = 1) != 12 { ret 6; }
    if exclusive.value != 12 { ret 7; }
    var consumed = Consumed { value: 0 };
    if consumed.digits(second = 2, first = 1) != 12 { ret 8; }
    var other = Other { value: 0 };
    if other.digits(2, 1) != 12 { ret 9; }
    if other.digits(first = 1, second = 2) != 12 { ret 10; }
    if Factory.digits(1, 2) != 12 { ret 11; }
    if Factory.digits(second = 2, first = 1) != 12 { ret 12; }
    var generic: Generic[i32] = Generic { value: 0 };
    if generic.digits(second = 2, first = 1) != 12 { ret 13; }
    var created: Generic[i32] = Generic.create[i32](second = 2, first = 1);
    if created.value != 1 { ret 14; }
    if forward[i32](0) != 12 { ret 15; }
    if forward[i64](0) != 12 { ret 16; }
    if helpers.digits(1, 2) != 12 { ret 17; }
    if helpers.digits(second = 2, first = 1) != 12 { ret 18; }
    if helpers.digits(first = 1, second = 2) != 12 { ret 19; }
    if helpers.generic[i32](second = 2, first = 1) != 12 { ret 20; }
    if imported(second = 2, first = 1) != 12 { ret 21; }
    var closure: fn() i32 = || helpers.digits(second = 2, first = 1);
    if closure() != 12 { ret 22; }
    if formatted("a", "bb") != 1 { ret 23; }
    if formatted(rest = "bb", first = "a") != 1 { ret 24; }
    ret 0;
}
"""

HELPERS = """pub fn digits(first: i32, second: i32) i32 { ret first * 10 + second; }
pub fn generic[T](first: i32, second: i32) i32 { ret first * 10 + second; }
"""


def run(command, root):
    result = subprocess.run(command, cwd=root, text=True, stdout=subprocess.PIPE,
                            stderr=subprocess.STDOUT, timeout=60)
    if result.returncode != 0:
        raise AssertionError(f"{command!r} returned {result.returncode}:\n{result.stdout}")


def check(qz, root):
    (root / "src").mkdir()
    (root / "quazi.toml").write_text(
        '[package]\nname="named_arguments"\nstd=false\ncrash_handler=false\n'
        '[build]\nentry="src/main.qz"\nsrc="src"\n'
    )
    (root / "src/main.qz").write_text(SOURCE)
    (root / "src/helpers.qz").write_text(HELPERS)
    common = ["--no-incremental", "--no-color", "--no-unicode", "--silent"]
    native = root / "native"
    run([str(qz), "build", *common, "-o", str(native)], root)
    run([str(native)], root)
    artifact = root / "program.qzi"
    run([str(qz), "build", *common, "-i", "-o", str(artifact)], root)
    assert artifact.read_bytes()[:4] == b"\x00QZI"
    portable = root / "portable"
    run([str(qz), "build", str(artifact), *common, "-o", str(portable)], root)
    run([str(portable)], root)
    coff = root / "program.obj"
    run([str(qz), "build", str(artifact), *common, "-c", "--target",
         "x86_64-windows", "-o", str(coff)], root)
    assert coff.read_bytes()[:2] == b"\x64\x86", "expected x86-64 COFF"
    print("PASS named arguments: receiver/static/module/generic/closure/variadic, native, QZI, COFF")


def imported_qzi(qz, root):
    library = root / "library"
    (library / "src").mkdir(parents=True)
    (library / "quazi.toml").write_text(
        '[package]\nname="original"\nstd=false\ncrash_handler=false\n'
        '[lib]\npath="src/lib.qz"\n'
    )
    (library / "src/lib.qz").write_text(HELPERS.split("pub fn generic")[0])
    artifact = root / "original.qzi"
    common = ["--no-incremental", "--no-color", "--no-unicode", "--silent"]
    run([str(qz), "build", "--lib", "-i", "-o", str(artifact), *common], library)
    shutil.rmtree(library / "src")
    consumer = root / "consumer"
    (consumer / "src").mkdir(parents=True)
    (consumer / "quazi.toml").write_text(
        '[package]\nname="consumer"\nstd=false\ncrash_handler=false\n'
        '[dependencies]\nrenamed={path="../original.qzi"}\n'
    )
    (consumer / "src/main.qz").write_text(
        'import renamed; import renamed.digits as alias; '
        'fn main() i32 { '
        'if renamed.digits(second=2, first=1) != 12 { ret 1; } '
        'if alias(second=2, first=1) != 12 { ret 2; } ret 0; }'
    )
    native = consumer / "native"
    run([str(qz), "build", *common, "-o", str(native)], consumer)
    run([str(native)], consumer)
    portable = consumer / "consumer.qzi"
    run([str(qz), "build", *common, "-i", "-o", str(portable)], consumer)
    run([str(qz), "build", str(portable), *common, "-o", str(native)], consumer)
    run([str(native)], consumer)
    coff = consumer / "consumer.obj"
    run([str(qz), "build", str(portable), *common, "-c", "--target",
         "x86_64-windows", "-o", str(coff)], consumer)
    assert coff.read_bytes()[:2] == b"\x64\x86", "expected x86-64 COFF"
    print("PASS named arguments: aliased scalar QZI library with defining source removed")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--qz", type=Path, default=Path("target/debug/qz"))
    args = parser.parse_args()
    qz = args.qz.resolve()
    with tempfile.TemporaryDirectory(prefix="quazi-named-arguments-") as directory:
        root = Path(directory)
        check(qz, root)
        imported_qzi(qz, root)


if __name__ == "__main__":
    main()
