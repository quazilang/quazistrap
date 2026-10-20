#!/usr/bin/env python3
"""Check shared operand control, lazy evaluation, and canonical logical bools."""

import argparse
import subprocess
import tempfile
from pathlib import Path


SOURCE = """fn logical_and(left: i32, right: i32) bool { ret left && right; }
fn logical_or(left: i32, right: i32) bool { ret left || right; }
fn nested(left: i32, middle: i32, right: i32) bool {
    ret (left && middle) || right;
}
unsafe fn tick(counter: *i32, value: i32) i32 {
    *counter = *counter + 1;
    ret value;
}
unsafe fn marked(counter: *i32, succeed: bool) Result[i32, i32] {
    *counter = *counter + 1;
    if succeed { ret Ok(3); }
    ret Err(7);
}
unsafe fn optional(counter: *i32, succeed: bool) Option[i32] {
    *counter = *counter + 1;
    if succeed { ret Some(3); }
    ret None;
}
unsafe fn guarded_and(counter: *i32, left: i32, succeed: bool) Result[bool, i32] {
    ret Ok(left && marked(counter, succeed)?);
}
unsafe fn guarded_or(counter: *i32, left: i32, succeed: bool) Result[bool, i32] {
    ret Ok(left || marked(counter, succeed)?);
}
unsafe fn guarded_option(counter: *i32, left: i32, succeed: bool) Option[bool] {
    ret Some(left && optional(counter, succeed)?);
}
unsafe fn eager(counter: *i32, succeed: bool) Result[i32, i32] {
    ret Ok(tick(counter, 2) + (-((marked(counter, succeed)?) as i32)) + tick(counter, 4));
}
fn select(first: bool, second: i32) bool { ret first; }
unsafe fn opaque_named(counter: *i32, succeed: bool) Result[bool, i32] {
    ret Ok(select(second = 9, first = !(marked(counter, succeed)? == 0)));
}
fn check_booleans() i32 {
    const folded_and = true && true;
    const integer_and = 2 && 3;
    const folded_or = false || true;
    const integer_or = 0 || 3;
    if folded_and != true { ret 1; }
    if folded_or != true { ret 2; }
    if logical_and(2, 3) != folded_and { ret 3; }
    if logical_or(0, 3) != folded_or { ret 4; }
    if integer_and != folded_and { ret 35; }
    if integer_or != folded_or { ret 36; }
    if logical_and(-2, -3) != true { ret 5; }
    if logical_and(2, 0) != false { ret 6; }
    if logical_or(0, -3) != true { ret 7; }
    if logical_or(-2, 0) != true { ret 8; }
    if nested(2, 3, 0) != true { ret 9; }
    if nested(0, 3, 4) != true { ret 10; }
    var closure: fn(i32) bool = |value| value && 3;
    if closure(2) != true { ret 11; }
    if closure(0) != false { ret 12; }
    ret 0;
}
fn check_results() i32 {
    var count: i32 = 0;
    unsafe {
        var skipped = guarded_and(&count, 0, false);
        if match skipped { Ok(value) => value != false, Err(error) => true, } { ret 13; }
        if count != 0 { ret 14; }
        var shortcut = guarded_or(&count, 2, false);
        if match shortcut { Ok(value) => value != true, Err(error) => true, } { ret 15; }
        if count != 0 { ret 16; }
        var passed = guarded_and(&count, 2, true);
        if match passed { Ok(value) => value != true, Err(error) => true, } { ret 17; }
        if count != 1 { ret 18; }
        var failed = guarded_or(&count, 0, false);
        if match failed { Ok(value) => true, Err(error) => error != 7, } { ret 19; }
        if count != 2 { ret 20; }
    }
    ret 0;
}
fn check_options() i32 {
    var count: i32 = 0;
    unsafe {
        var skipped_option = guarded_option(&count, 0, false);
        if match skipped_option { Some(value) => value != false, None => true, } { ret 21; }
        if count != 0 { ret 22; }
        var failed_option = guarded_option(&count, 2, false);
        if match failed_option { Some(value) => true, None => false, } { ret 23; }
        if count != 1 { ret 24; }
        var passed_option = guarded_option(&count, 2, true);
        if match passed_option { Some(value) => value != true, None => true, } { ret 25; }
        if count != 2 { ret 26; }
    }
    ret 0;
}
fn check_eager() i32 {
    var count: i32 = 0;
    unsafe {
        var failed_eager = eager(&count, false);
        if match failed_eager { Ok(value) => true, Err(error) => error != 7, } { ret 27; }
        if count != 2 { ret 28; }
        var passed_eager = eager(&count, true);
        if match passed_eager { Ok(value) => value != 3, Err(error) => true, } { ret 29; }
        if count != 5 { ret 30; }
        var named = opaque_named(&count, true);
        if match named { Ok(value) => value != true, Err(error) => true, } { ret 31; }
        if count != 6 { ret 32; }
        var failed_named = opaque_named(&count, false);
        if match failed_named { Ok(value) => true, Err(error) => error != 7, } { ret 33; }
        if count != 7 { ret 34; }
    }
    ret 0;
}
fn main() i32 {
    var result0 = check_booleans();
    if result0 != 0 { ret result0; }
    var result1 = check_results();
    if result1 != 0 { ret result1; }
    var result2 = check_options();
    if result2 != 0 { ret result2; }
    var result3 = check_eager();
    if result3 != 0 { ret result3; }
    ret 0;
}
"""


def run(command, root):
    result = subprocess.run(command, cwd=root, text=True, stdout=subprocess.PIPE,
                            stderr=subprocess.STDOUT, timeout=60)
    if result.returncode:
        raise AssertionError(f"{command!r} returned {result.returncode}:\n{result.stdout}")
    return result.stdout


def check(qz, root, before=None):
    (root / "src").mkdir()
    (root / "quazi.toml").write_text(
        '[package]\nname="expression_control"\nstd=false\ncrash_handler=false\n'
        '[build]\nentry="src/main.qz"\nsrc="src"\n'
    )
    (root / "src/main.qz").write_text(SOURCE)
    if before is not None:
        cached = root / "cached"
        cache_args = ["build", "-o", str(cached), "--no-color", "--no-unicode"]
        run([str(before), *cache_args], root)
        previous = subprocess.run([str(cached)], cwd=root, timeout=60, capture_output=True)
        assert previous.returncode == 3, "baseline must reproduce the logical bool regression"
        output = run([str(qz), *cache_args], root)
        assert "cache lookup miss" in " ".join(output.split()), output
        run([str(cached)], root)
        output = run([str(qz), *cache_args], root)
        assert "cache lookup hit" in " ".join(output.split()), output
        run([str(cached)], root)
        print("PASS expression control: old source cache invalidated, new exact hit preserves bool fix")
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
    print("PASS expression control: 36 checks, native, QZI-only, Windows COFF")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--qz", type=Path, default=Path("target/debug/qz"))
    parser.add_argument("--before-qz", type=Path,
                        help="optional preceding compiler for source-cache upgrade regression")
    args = parser.parse_args()
    qz = args.qz.resolve()
    before = args.before_qz.resolve() if args.before_qz else None
    with tempfile.TemporaryDirectory(prefix="quazi-expression-control-") as directory:
        check(qz, Path(directory), before)


if __name__ == "__main__":
    main()
