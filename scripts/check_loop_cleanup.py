#!/usr/bin/env python3
"""Verify loop cleanup with native destructor traces, QZI, COFF and cache upgrades."""

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
unsafe fn branch(trace: *i32, stop: bool) void {
    for {
        var first = mark(trace, 1);
        if stop { break; }
        var second = mark(trace, 2);
        break;
    }
}
unsafe fn choose(trace: *i32, enter: bool) Owner {
    for var item = mark(trace, 5); enter; { ret item; }
    ret mark(trace, 7);
}
unsafe fn advance(trace: *i32, index: i32) i32 {
    if index == 0 && *trace != 1 { ret 99; }
    if index == 1 && *trace != 11 { ret 99; }
    if index == 2 && *trace != 111 { ret 99; }
    ret index + 1;
}
unsafe fn each_slice(trace: *i32, ...values: i32) void {
    for value : &values { var item = mark(trace, value); continue; }
}
"""

# Each case returns before parameter/outer-scope cleanup. A decimal trace records
# the exact destruction order; the outer main checks again after the case returns.
CASES = {
    "break_nested_order": ("""
        var outside = mark(trace, 9);
        for { var first = mark(trace, 1); unsafe { var second = mark(trace, 2); break; } }
        if *trace != 21 { ret 1; }
    """, 219),
    "conditional_break_taken": ("branch(trace, true);", 1),
    "conditional_break_fallthrough": ("branch(trace, false);", 21),
    "continue_and_fallthrough": ("""
        var outside = mark(trace, 9);
        var index: i32 = 0;
        for index < 3 {
            var item = mark(trace, 1);
            index += 1;
            if index < 3 { continue; }
        }
        if *trace != 111 { ret 1; }
    """, 1119),
    "continue_before_update": ("""
        for var index = 0; index < 3; index = advance(trace, index) {
            var item = mark(trace, 1); continue;
        }
    """, 111),
    "nested_loop_boundaries": ("""
        var outside = mark(trace, 9);
        for var index = 0; index < 2; index += 1 {
            var outer = mark(trace, 1);
            for { var inner = mark(trace, 2); break; }
            if index == 0 && *trace != 2 { ret 1; }
            if index == 1 && *trace != 212 { ret 1; }
            continue;
        }
        if *trace != 2121 { ret 1; }
    """, 21219),
    "once_initializer_expression": (
        "var item = mark(trace, 5); for consume(item); false; {} if *trace != 5 { ret 1; }", 5),
    "once_initializer_binding": (
        "var item = mark(trace, 5); for var inner = item; false; {} if *trace != 5 { ret 1; }", 5),
    "initializer_loan_ends": (
        "var item = mark(trace, 5); for var reference = &item; false; {} consume(item);", 5),
    "initializer_zero_iterations": (
        "for var item = mark(trace, 5); false; {} if *trace != 5 { ret 1; }", 5),
    "initializer_break": ("""
        for var init = mark(trace, 5); true; { var body = mark(trace, 1); break; }
        if *trace != 15 { ret 1; }
    """, 15),
    "initializer_continue_and_exhaustion": ("""
        var index: i32 = 0;
        for var init = mark(trace, 5); index < 3; index += 1 {
            var body = mark(trace, 1); continue;
        }
        if *trace != 1115 { ret 1; }
    """, 1115),
    "initializer_return": ("""
        var outside = mark(trace, 9);
        for var init = mark(trace, 5); true; { var body = mark(trace, 1); ret 0; }
    """, 159),
    "returned_initializer_taken": ("var result = choose(trace, true); result.free();", 5),
    "returned_initializer_zero_iterations": (
        "var result = choose(trace, false); result.free();", 57),
    "initializer_shadow": ("""
        var item = mark(trace, 9);
        for var item = mark(trace, 1); false; {}
        item.free();
    """, 19),
    "body_shadow": ("""
        var item = mark(trace, 9);
        for { var item = mark(trace, 1); break; }
        item.free();
    """, 19),
    "range_continue": (
        "for index : 0..3 { var item = mark(trace, 1); continue; }", 111),
    "fixed_array_continue": ("""
        var values = [1, 2, 3];
        for value : &values { var item = mark(trace, value); continue; }
    """, 123),
    "fixed_array_break": ("""
        var values = [1, 2, 3];
        for value : &values { var item = mark(trace, value); if value == 2 { break; } }
    """, 12),
    "slice_continue": ("each_slice(trace, 1, 2, 3);", 123),
    "moved_body_local": (
        "for { var item = mark(trace, 1); consume(item); break; }", 1),
    "structural_body_local": (
        "for { var pair = Pair { child: mark(trace, 1) }; break; }", 1),
}


def source():
    functions = []
    checks = []
    for index, (name, (body, expected)) in enumerate(CASES.items(), 1):
        functions.append(f"unsafe fn {name}(trace: *i32) i32 {{ {body} ret 0; }}")
        checks.append(f"""unsafe {{ var trace: i32 = 0;
            if {name}(&trace) != 0 {{ ret {index}; }}
            if trace != {expected} {{ ret {index}; }} }}""")
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
        '[package]\nname="loop_cleanup"\nstd=false\ncrash_handler=false\n'
        '[build]\nentry="main.qz"\n'
    )
    entry = root / "main.qz"
    entry.write_text(source())
    native = root / "native"
    cache_args = ["build", "--no-color", "--no-unicode", "-o", str(native)]
    if before:
        # Keep the old fixture within the old borrow checker's accepted language:
        # the full matrix also includes once-only initializer moves it rejected.
        entry.write_text(PRELUDE + """unsafe fn test(trace: *i32) void {
            for { var item = mark(trace, 1); break; }
        }
        fn main() i32 {
            var trace: i32 = 0; unsafe { test(&trace); }
            if trace != 1 { ret 7; } ret 0;
        }""")
        run([str(before), *cache_args], root)
        old = subprocess.run([str(native)], cwd=root, timeout=60, capture_output=True)
        assert old.returncode == 7, "baseline must reproduce the missing destructor"
        output = run([str(qz), *cache_args], root)
        assert "cache lookup miss" in " ".join(output.split()), output
        run([str(native)], root)
        output = run([str(qz), *cache_args], root)
        assert "cache lookup hit" in " ".join(output.split()), output
        run([str(native)], root)
        print("PASS loop cleanup: old cache rebuilt, missing destructor fixed, exact hit runs")
        entry.write_text(source())
    common = ["--no-incremental", "--no-color", "--no-unicode", "--silent"]
    run([str(qz), "check"], root)
    run([str(qz), "build", *common, "-o", str(native)], root)
    run([str(native)], root)
    artifact = root / "program.qzi"
    run([str(qz), "build", *common, "-i", "-o", str(artifact)], root)
    entry.unlink()
    # Avoid loading the source manifest after removing its required entry.
    emission = directory / "artifact-only"
    emission.mkdir()
    portable = emission / "portable"
    run([str(qz), "build", str(artifact), *common, "-o", str(portable)], emission)
    run([str(portable)], emission)
    coff = emission / "program.obj"
    run([str(qz), "build", str(artifact), *common, "-c", "--target",
         "x86_64-windows", "-o", str(coff)], emission)
    assert coff.read_bytes()[:2] == b"\x64\x86"
    print(f"PASS loop cleanup: {len(CASES)} trace cases, native/source-removed QZI/Windows COFF")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--qz", type=Path, default=Path("target/debug/qz"))
    parser.add_argument("--before-qz", type=Path)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="quazi-loop-cleanup-") as directory:
        check(args.qz.resolve(), Path(directory),
              args.before_qz.resolve() if args.before_qz else None)


if __name__ == "__main__":
    main()
