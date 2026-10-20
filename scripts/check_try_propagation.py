#!/usr/bin/env python3
"""Check typed failure propagation, native/QZI execution and source-cache upgrades."""

import argparse
import re
import subprocess
import tempfile
from pathlib import Path


INVALID = {
    "scalar_return": "fn bad() i32 { ret failed()?; }",
    "void_return": "fn bad() void { const value = failed()?; }",
    "wrong_wrapper": "fn bad() Option[i32] { ret Some(failed()?); }",
    "error_conversion": "fn bad() Result[i32, f64] { ret Ok(failed()?); }",
    "error_signedness": "fn bad() Result[i32, u32] { ret Ok(failed()?); }",
    "generic_error": "fn bad[E, F](value: Result[i32, E]) Result[i32, F] { ret Ok(value?); }",
    "typed_closure": "fn bad() Result[i32, i32] { var f: fn() i32 = || failed()?; ret Ok(1); }",
    "inferred_closure": "fn bad() Result[i32, i32] { var f = || failed()?; ret Ok(1); }",
    "untyped_constructor": "fn bad() Option[i32] { ret Some(Option.Some(1)?); }",
}

PRELUDE = "fn failed() Result[i32, i32] { ret Err(7); }\n"
MAIN = "\nfn main() i32 { ret 0; }\n"
VALID = """type ErrorCode = i32;
type Input = Result[i32, ErrorCode];
type Output = Result[bool, i32];
type Maybe[T] = Option[T];
type Shared = &Option[i32];
fn present(value: Shared) bool {
    ret match value { Some(_) => true, None => false, };
}
fn result(succeed: bool) Input {
    if succeed { ret Ok(5); }
    ret Err(7);
}
fn optional(succeed: bool) Maybe[i32] {
    if succeed { ret Some(5); }
    ret None;
}
fn forward[E](value: Result[i32, E]) Result[bool, E] {
    ret Ok(value? > 0);
}
fn mapped(succeed: bool) Output {
    ret forward[i32](result(succeed));
}
fn option_map(succeed: bool) Maybe[bool] {
    var scalar: fn() i32 = || 1;
    ret Some(optional(succeed)? > scalar());
}
struct Mapper {}
impl Mapper {
    fn map(self: &Mapper, succeed: bool) Output {
        ret Ok(result(succeed)? > 0);
    }
}
fn main() i32 {
    var success = mapped(true);
    if match success { Ok(value) => !value, Err(error) => true, } { ret 1; }
    var failure = mapped(false);
    if match failure { Ok(value) => true, Err(error) => error != 7, } { ret 2; }
    var some = option_map(true);
    if match some { Some(value) => !value, None => true, } { ret 3; }
    var none = option_map(false);
    if match none { Some(value) => true, None => false, } { ret 4; }
    var mapper = Mapper {};
    var method_success = mapper.map(true);
    if match method_success { Ok(value) => !value, Err(error) => true, } { ret 5; }
    var method_failure = mapper.map(false);
    if match method_failure { Ok(value) => true, Err(error) => error != 7, } { ret 6; }
    var borrowed_some: Option[i32] = Some(1);
    if !present(&borrowed_some) { ret 7; }
    var borrowed_none: Option[i32] = None;
    if present(&borrowed_none) { ret 8; }
    ret 0;
}
"""


def invoke(command, root, succeeds=True):
    result = subprocess.run(command, cwd=root, text=True, stdout=subprocess.PIPE,
                            stderr=subprocess.STDOUT, timeout=60)
    assert (result.returncode == 0) == succeeds, (
        f"{command!r} returned {result.returncode}:\n{result.stdout}"
    )
    return re.sub(r"\x1b\[[0-9;]*m", "", result.stdout)


def project(root, source):
    root.mkdir()
    (root / "quazi.toml").write_text(
        '[package]\nname="try_propagation"\nstd=false\ncrash_handler=false\n'
        '[build]\nentry="main.qz"\n'
    )
    (root / "main.qz").write_text(source)


def check(qz, root, before):
    common = ["--no-incremental", "--no-color", "--no-unicode", "--silent"]
    for name, source in INVALID.items():
        case = root / name
        project(case, PRELUDE + source + MAIN)
        diagnostic = invoke([str(qz), "check"], case, succeeds=False)
        assert "error[S14]" in diagnostic and "`?`" in diagnostic, diagnostic
        for mode, suffix in [([], "native"), (["-i"], "qzi"),
                             (["-c", "--target", "x86_64-windows"], "obj")]:
            output = case / f"rejected.{suffix}"
            diagnostic = invoke([str(qz), "build", *common, *mode, "-o", str(output)],
                                case, succeeds=False)
            assert "error[S14]" in diagnostic and "`?`" in diagnostic, diagnostic
            assert not output.exists(), f"invalid source emitted {output}"
    print(f"PASS try propagation: {len(INVALID)} invalid programs rejected by check/native/QZI/COFF")

    valid = root / "valid"
    project(valid, VALID)
    invoke([str(qz), "check"], valid)
    binary = valid / "native"
    invoke([str(qz), "build", *common, "-o", str(binary)], valid)
    invoke([str(binary)], valid)
    artifact = valid / "program.qzi"
    invoke([str(qz), "build", *common, "-i", "-o", str(artifact)], valid)
    # Emit outside the source project's manifest, whose entry must exist when loaded.
    # The second build has no source file available to parse or compile.
    (valid / "main.qz").unlink()
    portable = valid / "portable"
    invoke([str(qz), "build", str(artifact), *common, "-o", str(portable)], root)
    invoke([str(portable)], valid)
    coff = valid / "program.obj"
    invoke([str(qz), "build", str(artifact), *common, "-c", "--target",
            "x86_64-windows", "-o", str(coff)], root)
    assert coff.read_bytes()[:2] == b"\x64\x86"
    print("PASS try propagation: 8 success/failure/discriminant checks, aliases/generics/methods, native/QZI-only/COFF")

    if before is not None:
        cached = root / "cached"
        project(cached, PRELUDE + INVALID["scalar_return"] + MAIN)
        old_output = cached / "old"
        args = ["build", "--no-color", "--no-unicode", "-o", str(old_output)]
        invoke([str(before), *args], cached)
        new_output = old_output
        old_output.unlink()
        diagnostic = invoke([str(qz), *args[:-1], str(new_output)], cached, succeeds=False)
        assert "cache lookup miss" in " ".join(diagnostic.split()), diagnostic
        assert "error[S14]" in diagnostic and "`?`" in diagnostic, diagnostic
        assert not new_output.exists()
        # A subsequent valid source must still compile and take a correct exact hit.
        (cached / "main.qz").write_text(VALID)
        invoke([str(qz), *args[:-1], str(new_output)], cached)
        invoke([str(new_output)], cached)
        diagnostic = invoke([str(qz), *args[:-1], str(new_output)], cached)
        assert "cache lookup hit" in " ".join(diagnostic.split()), diagnostic
        invoke([str(new_output)], cached)
        print("PASS try propagation: old invalid source cache rejected; valid exact hit runs")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--qz", type=Path, default=Path("target/debug/qz"))
    parser.add_argument("--before-qz", type=Path)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="quazi-try-") as directory:
        check(args.qz.resolve(), Path(directory),
              args.before_qz.resolve() if args.before_qz else None)


if __name__ == "__main__":
    main()
