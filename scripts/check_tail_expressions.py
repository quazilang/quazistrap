#!/usr/bin/env python3
"""Check D-017 tail values and `if` expressions: diagnostics, native and QZI-only execution."""

import argparse
import re
import subprocess
import tempfile
from pathlib import Path


# Each invalid program must fail `check` and every emission mode with this code.
INVALID = {
    "inner_missing_semicolon": ("E02", "fn f() i32 { for (true) { 1 } ret 0; }"),
    "statement_if_value": ("E02", "fn f(x: i32) i32 { if (x > 0) { 1 } else { 2 } ret 3; }"),
    "value_if_without_else": ("E02", "fn f(x: i32) i32 { const a: i32 = if (x > 0) { 1 }; ret a; }"),
    "tail_if_without_else": ("E02", "fn f(x: i32) i32 { if (x > 0) { 1 } }"),
    "statement_in_branch": ("E02", "fn f(x: i32) i32 { ret if (x > 0) { const y = 2; y } else { 0 }; }"),
    "branch_type_mismatch": ("S01", "fn f(x: i32) i32 { ret if (x > 0) { 1 } else { \"no\" }; }"),
    "tail_type_mismatch": ("S01", "fn f(x: i32) i32 { \"text\" }"),
}
MAIN = "\nfn main() i32 { ret 0; }\n"

VALID = """fn add(a: i32, b: i32) i32 { a + b }
fn max(a: i32, b: i32) i32 {
    if (a > b) {
        a
    } else {
        b
    }
}
fn abs(x: i32) i32 {
    const result: i32 = if (x < 0) { -x } else { x };
    result
}
fn clamp(x: i32, low: i32, high: i32) i32 {
    if (x < low) {
        ret low;
    }
    if (x > high) {
        ret high;
    }
    x
}
fn sign(x: i32) i32 { if (x < 0) { -1 } else if (x == 0) { 0 } else { 1 } }
fn classify(x: i32) i32 {
    match x {
        0 => 10,
        _ if x > 0 => 20,
        _ => 30,
    }
}
fn nested(a: bool, b: bool) i32 {
    if (a) {
        if (b) { 1 } else { 2 }
    } else {
        const base: i32 = 3;
        base + if (b) { 1 } else { 0 }
    }
}
fn twice(x: i32) i32 { unsafe { x * 2 } }
fn truthy(x: i32) i32 { if (x) { 1 } else { 0 } }
fn bump(value: &i32!) void { *value = *value + 1 }
fn main() i32 {
    if (add(2, 3) != 5) { ret 1; }
    if (max(4, 9) != 9 || max(9, 4) != 9) { ret 2; }
    if (abs(-7) != 7 || abs(7) != 7) { ret 3; }
    if (clamp(15, 0, 10) != 10 || clamp(-5, 0, 10) != 0 || clamp(5, 0, 10) != 5) { ret 4; }
    if (sign(-5) != -1 || sign(0) != 0 || sign(8) != 1) { ret 5; }
    if (classify(0) != 10 || classify(4) != 20 || classify(-4) != 30) { ret 6; }
    if (nested(true, true) != 1 || nested(true, false) != 2) { ret 7; }
    if (nested(false, true) != 4 || nested(false, false) != 3) { ret 8; }
    if (twice(21) != 42) { ret 9; }
    if (truthy(5) != 1 || truthy(0) != 0) { ret 10; }
    var counter: i32 = 0;
    bump(&counter!);
    bump(&counter!);
    if (counter != 2) { ret 11; }
    0
}
"""

# Owned results must move out of the selected branch exactly once.
OWNED = """import std.io;
fn label(flag: bool) String {
    if (flag) {
        String.from("on")
    } else {
        String.from("off")
    }
}
fn pick(flag: bool) String {
    const text: String = if (flag) { String.from("yes") } else { String.from("no") };
    text
}
fn checked(x: i32) i32 { if (x >= 0) { x } else { panic("negative") } }
fn main() i32 {
    const on: String = label(true);
    const off: String = label(false);
    const yes: String = pick(true);
    const no: String = pick(false);
    io.println("{} {} {} {} {}", on.as_str(), off.as_str(), yes.as_str(), no.as_str(), checked(3));
    if (checked(-1) != 0) { ret 2; }
    0
}
"""


def invoke(command, root, succeeds=True):
    result = subprocess.run(command, cwd=root, text=True, stdout=subprocess.PIPE,
                            stderr=subprocess.STDOUT, timeout=120)
    assert (result.returncode == 0) == succeeds, (
        f"{command!r} returned {result.returncode}:\n{result.stdout}"
    )
    return re.sub(r"\x1b\[[0-9;]*m", "", result.stdout)


def project(root, source, std=False):
    root.mkdir()
    settings = "" if std else "std=false\ncrash_handler=false\n"
    (root / "quazi.toml").write_text(
        f'[package]\nname="tail_expressions"\n{settings}[build]\nentry="main.qz"\n'
    )
    (root / "main.qz").write_text(source)


def run_all_modes(qz, case, root, common):
    binary = case / "native"
    invoke([str(qz), "build", *common, "-o", str(binary)], case)
    native = subprocess.run([str(binary)], cwd=case, text=True, capture_output=True, timeout=60)
    artifact = case / "program.qzi"
    invoke([str(qz), "build", *common, "-i", "-o", str(artifact)], case)
    # The second build has no source file available to parse or compile.
    (case / "main.qz").unlink()
    portable = case / "portable"
    invoke([str(qz), "build", str(artifact), *common, "-o", str(portable)], root)
    qzi = subprocess.run([str(portable)], cwd=case, text=True, capture_output=True, timeout=60)
    coff = case / "program.obj"
    invoke([str(qz), "build", str(artifact), *common, "-c", "--target",
            "x86_64-windows", "-o", str(coff)], root)
    assert coff.read_bytes()[:2] == b"\x64\x86"
    return native, qzi


def check(qz, root):
    common = ["--no-incremental", "--no-color", "--no-unicode", "--silent"]
    for name, (code, source) in INVALID.items():
        case = root / name
        project(case, source + MAIN)
        diagnostic = invoke([str(qz), "check"], case, succeeds=False)
        assert f"error[{code}]" in diagnostic, f"{name}: {diagnostic}"
        for mode, suffix in [([], "native"), (["-i"], "qzi"),
                             (["-c", "--target", "x86_64-windows"], "obj")]:
            output = case / f"rejected.{suffix}"
            invoke([str(qz), "build", *common, *mode, "-o", str(output)], case, succeeds=False)
            assert not output.exists(), f"invalid source emitted {output}"
    print(f"PASS tail expressions: {len(INVALID)} invalid programs rejected by check/native/QZI/COFF")

    case = root / "valid"
    project(case, VALID)
    invoke([str(qz), "check"], case)
    for result in run_all_modes(qz, case, root, common):
        assert result.returncode == 0, f"failing check {result.returncode}"
    print("PASS tail expressions: 11 value checks native/source-removed QZI/Windows COFF")

    case = root / "owned"
    project(case, OWNED, std=True)
    invoke([str(qz), "check"], case)
    for result in run_all_modes(qz, case, root, common):
        # The second `checked` call panics after the owned values were printed.
        assert result.stdout.startswith("on off yes no 3\n"), result.stdout
        assert result.returncode != 0 and result.returncode != 2, result.returncode
    print("PASS tail expressions: owned String branches and `!` branch native/source-removed QZI")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--qz", type=Path, default=Path("target/debug/qz"))
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="quazi-tail-") as directory:
        check(args.qz.resolve(), Path(directory))


if __name__ == "__main__":
    main()
