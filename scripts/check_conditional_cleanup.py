#!/usr/bin/env python3
"""Check path-dependent owner cleanup, portable QZI and source-cache upgrades."""

import argparse
import subprocess
import tempfile
from pathlib import Path


PRELUDE = """struct Owner { trace: *i32, digit: i32 }
impl Owner {
    fn free(self: Owner) void {
        unsafe { *self.trace = *self.trace * 10 + self.digit; }
    }
}
struct Pair { child: Owner }
unsafe fn mark(trace: *i32, digit: i32) Owner {
    ret Owner { trace: trace, digit: digit };
}
fn consume(value: Owner) void {}
fn take(value: Owner, scalar: i32) void {}
fn fail() Result[i32, i32] { ret Err(7); }
unsafe fn initialize(trace: *i32, enter: bool) void {
    var item: Owner;
    if enter { item = mark(trace, 1); }
}
unsafe fn move_branch(trace: *i32, enter: bool) void {
    var item = mark(trace, 1);
    if enter { consume(item); }
}
unsafe fn replace(trace: *i32, enter: bool) void {
    var item = mark(trace, 1);
    if enter { item = mark(trace, 2); }
}
unsafe fn choose(trace: *i32, first: bool) Owner {
    var left = mark(trace, 1);
    var right = mark(trace, 2);
    ret match first { true => left, false => right, };
}
unsafe fn pending(trace: *i32, first: bool) Result[i32, i32] {
    var left = mark(trace, 1);
    var right = mark(trace, 2);
    take(match first { true => left, false => right, }, fail()?);
    ret Ok(0);
}
unsafe fn grouped(trace: *i32) Result[i32, i32] {
    var item = mark(trace, 1);
    take((item), fail()?);
    ret Ok(0);
}
unsafe fn grouped_producer(trace: *i32) Result[i32, i32] {
    take((mark(trace, 1)), fail()?);
    ret Ok(0);
}
unsafe fn cast_producer(trace: *i32) Result[i32, i32] {
    take((mark(trace, 1) as Owner), fail()?);
    ret Ok(0);
}
unsafe fn pending_producers(trace: *i32, first: bool) Result[i32, i32] {
    take(match first { true => mark(trace, 1), false => mark(trace, 2), }, fail()?);
    ret Ok(0);
}
unsafe fn method_pending(trace: *i32) Result[i32, i32] {
    var item = mark(trace, 1);
    item.accept(fail()?);
    ret Ok(0);
}
impl Owner { fn accept(self: Owner, scalar: i32) void {} }
fn parameters(first: Owner, scalar: i32, second: Owner, ...values: i32) i32 {
    var result = scalar;
    for value : &values { result += value; }
    ret result;
}
"""

# A digit is appended at each destruction; each case is checked after returning
# so that early exits, unselected values and final scope cleanup all contribute.
CASES = {
    "initialization_skipped": ("initialize(trace, false);", 0),
    "initialization_taken": ("initialize(trace, true);", 1),
    "move_skipped": ("move_branch(trace, false);", 1),
    "move_taken": ("move_branch(trace, true);", 1),
    "replace_skipped": ("replace(trace, false);", 1),
    "replace_taken": ("replace(trace, true);", 12),
    "uninitialized_replace_skipped": ("""
        var item: Owner;
        if false { item = mark(trace, 1); }
        item = mark(trace, 2);
    """, 2),
    "both_initializers": ("""
        var item: Owner;
        if *trace == 0 { item = mark(trace, 1); }
        else { item = mark(trace, 2); }
    """, 1),
    "skipped_loop_initialization": ("""
        var item: Owner;
        for *trace != 0 { item = mark(trace, 1); break; }
    """, 0),
    "loop_initialization_taken": ("""
        var item: Owner;
        for *trace == 0 { item = mark(trace, 1); break; }
    """, 1),
    "loop_guard_resets": ("""
        for var index = 0; index < 3; index += 1 {
            var item: Owner;
            if index == 1 { item = mark(trace, 1); }
            continue;
        }
    """, 1),
    "match_return_first": ("var result = choose(trace, true); result.free();", 21),
    "match_return_second": ("var result = choose(trace, false); result.free();", 12),
    "match_binding": ("""
        var left = mark(trace, 1);
        var right = mark(trace, 2);
        var result = match *trace == 0 { true => left, false => right, };
    """, 12),
    "match_call": ("""
        var left = mark(trace, 1);
        var right = mark(trace, 2);
        consume(match *trace != 0 { true => left, false => right, });
    """, 21),
    "match_self_replacement": ("""
        var item = mark(trace, 1);
        item = match *trace != 0 { true => item, false => mark(trace, 2), };
    """, 12),
    "match_self_retained": ("""
        var item = mark(trace, 1);
        item = match *trace == 0 { true => item, false => mark(trace, 2), };
    """, 1),
    "pending_match_first": ("var result = pending(trace, true);", 12),
    "pending_match_second": ("var result = pending(trace, false);", 21),
    "pending_group": ("var result = grouped(trace);", 1),
    "pending_group_producer": ("var result = grouped_producer(trace);", 1),
    "pending_cast_producer": ("var result = cast_producer(trace);", 1),
    "pending_fresh_match_first": ("var result = pending_producers(trace, true);", 1),
    "pending_fresh_match_second": ("var result = pending_producers(trace, false);", 2),
    "pending_receiver": ("var result = method_pending(trace);", 1),
    "scalar_shadow": ("""
        var item = mark(trace, 1);
        unsafe { var item: i32 = 3; var copied = item; }
    """, 1),
    "owner_shadow": ("""
        var item = mark(trace, 1);
        unsafe { var item = mark(trace, 2); consume(item); }
    """, 21),
    "structural_conditional": ("""
        var pair: Pair;
        if *trace != 0 { pair = Pair { child: mark(trace, 1) }; }
    """, 0),
    "structural_initialized": ("""
        var pair: Pair;
        if *trace == 0 { pair = Pair { child: mark(trace, 1) }; }
    """, 1),
    "parameter_prefix": ("""
        if parameters(mark(trace, 1), 3, mark(trace, 2), 4, 5) != 12 { ret 1; }
    """, 21),
    "zero_argument_factory": ("""
        var factory = || 7;
        var first = factory();
        var second = factory();
        if first + second != 14 { ret 1; }
    """, 0),
}


def source():
    functions = []
    checks = []
    for index, (name, (body, expected)) in enumerate(CASES.items(), 1):
        functions.append(f"unsafe fn {name}(trace: *i32) i32 {{ {body} ret 0; }}")
        functions.append(f"""unsafe fn check_{name}() bool {{ var trace: i32 = 0;
            if {name}(&trace) != 0 {{ ret false; }}
            ret trace == {expected}; }}""")
        checks.append(f"unsafe {{ if !check_{name}() {{ ret {index}; }} }}")
    return PRELUDE + "\n".join(functions) + "\nfn main() i32 {" + "\n".join(checks) + "ret 0; }\n"


def run(command, root):
    result = subprocess.run(command, cwd=root, text=True, stdout=subprocess.PIPE,
                            stderr=subprocess.STDOUT, timeout=60)
    assert result.returncode == 0, f"{command!r} exited {result.returncode}:\n{result.stdout}"
    return result.stdout


def check(qz, directory, before):
    root = directory / "source"
    root.mkdir()
    (root / "quazi.toml").write_text(
        '[package]\nname="conditional_cleanup"\nstd=false\ncrash_handler=false\n'
        '[build]\nentry="main.qz"\n'
    )
    entry = root / "main.qz"
    native = root / "native"
    cache_args = ["build", "--no-color", "--no-unicode", "-o", str(native)]
    if before:
        entry.write_text(PRELUDE + """fn main() i32 {
            var trace: i32 = 0; unsafe { move_branch(&trace, false); }
            if trace != 1 { ret 7; } ret 0;
        }""")
        run([str(before), *cache_args], root)
        old = subprocess.run([str(native)], cwd=root, timeout=60, capture_output=True)
        assert old.returncode == 7, "baseline must reproduce the skipped-branch owner leak"
        output = run([str(qz), *cache_args], root)
        assert "cache lookup miss" in " ".join(output.split()), output
        run([str(native)], root)
        output = run([str(qz), *cache_args], root)
        assert "cache lookup hit" in " ".join(output.split()), output
        run([str(native)], root)
        print("PASS conditional cleanup: baseline leak, cache rebuild and exact-hit execution")
    entry.write_text(source())
    common = ["--no-incremental", "--no-color", "--no-unicode", "--silent"]
    run([str(qz), "check"], root)
    run([str(qz), "build", *common, "-o", str(native)], root)
    run([str(native)], root)
    artifact = root / "program.qzi"
    run([str(qz), "build", *common, "-i", "-o", str(artifact)], root)
    entry.unlink()
    emission = directory / "artifact-only"
    emission.mkdir()
    portable = emission / "portable"
    run([str(qz), "build", str(artifact), *common, "-o", str(portable)], emission)
    run([str(portable)], emission)
    coff = emission / "program.obj"
    run([str(qz), "build", str(artifact), *common, "-c", "--target",
         "x86_64-windows", "-o", str(coff)], emission)
    assert coff.read_bytes()[:2] == b"\x64\x86"
    print(f"PASS conditional cleanup: {len(CASES)} traces, native/source-removed QZI/Windows COFF")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--qz", type=Path, default=Path("target/debug/qz"))
    parser.add_argument("--before-qz", type=Path)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="quazi-conditional-cleanup-") as directory:
        check(args.qz.resolve(), Path(directory),
              args.before_qz.resolve() if args.before_qz else None)


if __name__ == "__main__":
    main()
