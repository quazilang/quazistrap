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


# Calls through `&dyn Trait` (shared method) and `&dyn Trait!` (exclusive
# method) dispatch through the referenced value's vtable.
REFERENCES = """pub struct Counter { padding: i64, value: i32 }
pub struct Pair { left: i32, right: i32 }
pub trait Readable { fn read(self: &Self) i32; fn bump(self: &Self!) void; }
impl Readable for Counter {
    fn read(self: &Counter) i32 { ret self.value; }
    fn bump(self: &Counter!) void { self.value = self.value + 1; }
}
impl Readable for Pair {
    fn read(self: &Pair) i32 { ret self.left + self.right; }
    fn bump(self: &Pair!) void { self.left = self.left + 10; }
}
fn shared(value: &dyn Readable) i32 { ret value.read(); }
fn exclusive(value: &dyn Readable!) i32 { value.bump(); ret value.read(); }
fn main() i32 {
    var counter: dyn Readable = Counter { padding: 1, value: 41 };
    var pair: dyn Readable = Pair { left: 3, right: 4 };
    if (shared(&counter) != 41 || shared(&pair) != 7) { ret 1; }
    const first: i32 = exclusive(&counter!);
    const second: i32 = exclusive(&pair!);
    if (first != 42 || second != 17) { ret 2; }
    if (counter.read() != 42 || pair.read() != 17) { ret 3; }
    ret 0;
}
"""

# `&Concrete` passed for `&dyn Trait` borrows the value through a trait object
# released after the call (the value is not destroyed: a second destruction
# of Res would fault), including `&T` with `T: Trait` and a generic type; a value of
# a bounded type parameter converts to `dyn Trait`; variadic `&dyn` arguments,
# a `str` result derived through the object, and two instantiations of one
# generic type (separate vtables). Exit 0 checked.
BORROWED = """trait Show { fn show(self: &Self) i32; }
struct Res { id: i32 }
impl Res { fn free(self: Res) void { self.id = 0; } }
impl Show for Res { fn show(self: &Res) i32 { ret self.id; } }
struct Wrap[T] { inner: T }
impl Show for Wrap[T] { fn show(self: &Wrap[T]) i32 { ret 1000; } }
fn describe(item: &dyn Show) i32 { ret item.show(); }
fn both(a: &dyn Show, b: &dyn Show) i32 { ret a.show() + b.show(); }
fn many(first: i32, ...rest: &dyn Show) i32 { var total: i32 = first; for item : rest { total = total + item.show(); } ret total; }
trait Named { fn name(self: &Self) str; }
impl Named for Res { fn name(self: &Res) str { ret "res"; } }
fn title(item: &dyn Named) str { ret item.name(); }
fn forward(item: &Res) i32 { ret describe(item); }
fn via[T: Show](item: &T) i32 { ret describe(item); }
fn owned[T: Show](item: T) i32 { const object: dyn Show = item; ret object.show(); }
struct Printer { base: i32 }
impl Printer { fn print(self: &Printer, item: &dyn Show) i32 { ret self.base + item.show(); } }
fn main() i32 {
    const res = Res { id: 1 };
    const wrap: Wrap[i64] = Wrap { inner: 5 };
    var total: i32 = 0;
    for step : 0..100 { total = total + describe(&res); }
    if (total != 100 || both(&res, &wrap) != 1001 || forward(&res) != 1) { ret 1; }
    if (via(&res) != 1 || via(&wrap) != 1000) { ret 2; }
    const narrow: Wrap[i32] = Wrap { inner: 6 };
    if (describe(&wrap) + describe(&narrow) != 2000 || many(1, &res, &wrap) != 1002) { ret 5; }
    const label: str = title(&res);
    if (label != "res") { ret 6; }
    const printer = Printer { base: 10 };
    if (printer.print(&res) != 11) { ret 3; }
    if (owned(Res { id: 7 }) != 7) { ret 4; }
    ret res.show() - 1;
}
"""

# The trait object of a borrowed argument is released after the full
# expression, so a call result that may borrow from it is rejected.
BORROWED_ESCAPE = """trait Show { fn show(self: &Self) i32; }
struct Counter { value: i32 }
impl Show for Counter { fn show(self: &Counter) i32 { ret self.value; } }
fn same(item: &dyn Show) &dyn Show { ret item; }
fn main() i32 { const counter = Counter { value: 3 }; const view = same(&counter); ret view.show(); }
"""


# Methods reached only through trait objects (never called directly) must
# still be compiled for the vtables: a plain conversion, `Array[dyn T]` and
# `Box[dyn T]` failed with "missing vtable implementation".
UNCALLED = """trait Show { fn show(self: &Self) i32; }
struct C { v: i32 }
impl Show for C { fn show(self: &C) i32 { ret self.v; } }
fn take(s: dyn Show) i32 { ret 3; }
fn main() i32 {
    var items: Array[dyn Show] = Array.new();
    items.push(C { v: 1 });
    const boxed: Box[dyn Show] = Box.new(C { v: 2 });
    const plain: dyn Show = C { v: 4 };
    if (take(C { v: 3 }) != 3 || items.len() != 1) { ret 1; }
    ret 0;
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
    assert artifact.read_bytes()[:5] == b"\x00QZI\x12", "expected a QZI (format 18)"
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


def reference_dispatch(qz, root):
    project(root, "reference_dispatch", REFERENCES)
    portable_outputs(qz, root, "references")
    print("PASS dyn through references: &dyn shared and &dyn! exclusive methods, native, QZI-only, COFF")


def borrowed_dispatch(qz, root):
    project(root / "ok", "borrowed_dispatch", BORROWED)
    portable_outputs(qz, root / "ok", "borrowed")
    project(root / "escape", "borrowed_escape", BORROWED_ESCAPE)
    output = run(qz, root / "escape", "build", "--no-incremental", expected=1)
    assert "error[S10]" in output and "lives only for this call" in output, output
    print("PASS borrowed &dyn arguments and generic dyn conversions; escaping result rejected")


def uncalled_dispatch(qz, root):
    project(root, "uncalled_dispatch", UNCALLED)
    # `Array` and `Box` need the default library roots.
    (root / "quazi.toml").write_text('[package]\nname = "uncalled_dispatch"\n')
    portable_outputs(qz, root, "uncalled")
    print("PASS vtable methods never called directly: conversion, Array[dyn T], Box[dyn T]")


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
        reference_dispatch(qz, root / "references")
        borrowed_dispatch(qz, root / "borrowed")
        uncalled_dispatch(qz, root / "uncalled")
        aliased_library_dispatch(qz, root / "aliased")


if __name__ == "__main__":
    main()
