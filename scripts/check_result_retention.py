#!/usr/bin/env python3
"""Check aggregate result retention diagnostics and safe native execution."""

import argparse
import subprocess
import tempfile
from pathlib import Path


VIEW = """struct View { value: str }
fn wrap(value: str) View { ret View { value: value }; }
"""

NEGATIVE_CASES = {
    "raw_helper_copy": """struct View { value: str }
@intrinsic("quazi.malloc") unsafe fn allocate(n: usize) *u8;
@intrinsic("quazi.memcpy") unsafe fn copy(dst: *u8, src: *u8, n: usize) *u8;
unsafe fn storage() *u8 { ret allocate(8); }
fn wrap(value: str) View {
    var result: str = "";
    unsafe {
        var source = allocate(8);
        *(source as *str) = value;
        var destination = storage();
        copy(destination, source, 8);
        result = *(destination as *str);
    }
    ret View { value: result };
}
fn main() i32 {
    var owner = "retained".to_string();
    const view = wrap(owner.as_str());
    owner.free();
    ret view.value.len() as i32;
}
""",
    "unknown_pointer_alias": """struct View { value: str }
@intrinsic("quazi.malloc") unsafe fn allocate(n: usize) *u8;
unsafe fn read_after_write(left: *str, right: *str, value: str) str {
    *left = value;
    ret *right;
}
fn wrap(value: str) View {
    var result: str = "";
    unsafe {
        var pointer = allocate(8);
        result = read_after_write(pointer, pointer, value);
    }
    ret View { value: result };
}
fn main() i32 {
    var owner = "retained".to_string();
    const view = wrap(owner.as_str());
    owner.free();
    ret view.value.len() as i32;
}
""",
    "legacy_receiver_forwarding": """struct View { value: str }
struct Holder { value: str }
impl Holder { fn view(receiver: Holder) str { ret receiver.value; } }
fn wrap(value: str) View {
    const holder = Holder { value: value };
    ret View { value: holder.view() };
}
fn main() i32 {
    var owner = "owner".to_string();
    const view = wrap(owner.as_str());
    owner.free();
    ret view.value.len() as i32;
}
""",
    "qualified_enum_local_return": VIEW + """
enum Choice { Some(View), None }
fn leak() Choice {
    var owner = "owner".to_string();
    ret Choice.Some(wrap(owner.as_str()));
}
fn main() i32 { ret 0; }
""",
    "free_retained_owner": VIEW + """
fn main() i32 {
    var owner = "owner".to_string();
    const view = wrap(owner.as_str());
    owner.free();
    ret view.value.len() as i32;
}
""",
    "move_retained_owner": VIEW + """
fn release(owner: String) void { owner.free(); }
fn main() i32 {
    var owner = "owner".to_string();
    const view = wrap(owner.as_str());
    release(owner);
    ret view.value.len() as i32;
}
""",
    "return_local_owner_view": VIEW + """
fn leak() View {
    var owner = "owner".to_string();
    ret wrap(owner.as_str());
}
fn main() i32 { const view = leak(); ret view.value.len() as i32; }
""",
    "nested_wrapper": VIEW + """
struct Outer { inner: View }
fn nested(value: str) Outer { ret Outer { inner: wrap(value) }; }
fn main() i32 {
    var owner = "owner".to_string();
    const view = nested(owner.as_str());
    owner.free();
    ret view.inner.value.len() as i32;
}
""",
    "enum_payload_match": VIEW + """
enum Choice { Some(View), None }
fn boxed(value: str) Choice { ret Choice.Some(wrap(value)); }
fn main() i32 {
    var owner = "owner".to_string();
    const choice = boxed(owner.as_str());
    owner.free();
    ret match choice {
        Some(view) => view.value.len() as i32,
        None => 0,
    };
}
""",
    "generic_wrapper": """struct Holder[T] { value: T }
fn wrap[T](value: T) Holder[T] { ret Holder { value: value }; }
fn main() i32 {
    var owner = "owner".to_string();
    const view = wrap[str](owner.as_str());
    owner.free();
    ret view.value.len() as i32;
}
""",
    "forwarding_helper": VIEW + """
fn forward(value: str) View { const result = wrap(value); ret result; }
fn main() i32 {
    var owner = "owner".to_string();
    const view = forward(owner.as_str());
    owner.free();
    ret view.value.len() as i32;
}
""",
    "outward_assignment": VIEW + """
fn main() i32 {
    var view = View { value: "" };
    if true {
        var owner = "owner".to_string();
        view = wrap(owner.as_str());
    }
    ret view.value.len() as i32;
}
""",
}

POSITIVE_CASES = {
    "owned_range_copy": """fn copy(value: str) String { ret value[1:4]; }
fn main() i32 {
    var owner = "abcdef".to_string();
    const copied = copy(owner.as_str());
    owner.free();
    if copied.as_str() != "bcd" { ret 1; }
    ret 0;
}
""",
    "named_unretained_argument": """struct View { value: str }
fn wrap(first: str, ignored: str) View { ret View { value: first }; }
fn main() i32 {
    var first = "owner".to_string();
    var second = "other".to_string();
    const view = wrap(ignored = second.as_str(), first = first.as_str());
    second.free();
    if view.value.len() != 5 { ret 1; }
    ret 0;
}
""",
    "unretained_argument": """struct View { value: str }
fn wrap(first: str, ignored: str) View { ret View { value: first }; }
fn main() i32 {
    var first = "owner".to_string();
    var second = "other".to_string();
    const view = wrap(first.as_str(), second.as_str());
    second.free();
    if view.value.len() != 5 { ret 1; }
    ret 0;
}
""",
    "static_aggregate_return": VIEW + """
fn produce() View { ret wrap("literal"); }
fn main() i32 {
    const view = produce();
    if view.value.len() != 7 { ret 1; }
    ret 0;
}
""",
    "synchronous_observer": VIEW + """
fn count(value: str) usize {
    const view = View { value: value };
    ret view.value.len();
}
fn main() i32 {
    var owner = "owner".to_string();
    if count(owner.as_str()) != 5 { ret 1; }
    owner.free();
    ret 0;
}
""",
    "aggregate_scope_ends": VIEW + """
fn main() i32 {
    var owner = "owner".to_string();
    if true {
        const view = wrap(owner.as_str());
        if view.value.len() != 5 { ret 1; }
    }
    owner.free();
    ret 0;
}
""",
    "owned_copy": """fn copy(value: str) String { ret value.to_string(); }
fn main() i32 {
    var owner = "owner".to_string();
    const copied = copy(owner.as_str());
    owner.free();
    if copied.len() != 5 { ret 1; }
    ret 0;
}
""",
}


def run(command, root):
    return subprocess.run(
        list(map(str, command)), cwd=root, text=True,
        stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=60,
    )


def check_case(qz, root, name, source, *, reject):
    (root / "src").mkdir(parents=True)
    (root / "quazi.toml").write_text(
        f'[package]\nname = "{name}"\ncrash_handler = false\n'
    )
    (root / "src/main.qz").write_text(source)
    checked = run([qz, "check"], root)
    binary = root / "retention-test"
    built = run([
        qz, "build", "-o", binary, "--no-incremental", "--no-progress",
        "--no-color", "--no-unicode",
    ], root)
    artifact = root / "retention.qzi"
    emitted = run([
        qz, "build", "-i", "-o", artifact, "--no-incremental", "--no-progress",
        "--no-color", "--no-unicode",
    ], root)
    for phase, result in [("check", checked), ("build", built), ("QZI emission", emitted)]:
        if reject:
            assert result.returncode == 1 and "S10" in result.stdout, (
                f"{name}: {phase} must reject with S10; exit={result.returncode}\n"
                f"{result.stdout}\nSource:\n{source}"
            )
            assert str(root / "src/main.qz") in result.stdout, (
                f"{name}: {phase} has no diagnostic in the regression source\n{result.stdout}"
            )
        else:
            assert result.returncode == 0, (
                f"{name}: {phase} failed ({result.returncode})\n{result.stdout}\n"
                f"Source:\n{source}"
            )
    if reject:
        assert not artifact.exists(), f"{name}: unsafe source emitted QZI"
    else:
        executed = run([binary], root)
        assert executed.returncode == 0, (
            f"{name}: native execution failed ({executed.returncode})\n"
            f"{executed.stdout}\nSource:\n{source}"
        )
        replayed = run([qz, "run", artifact, "--no-progress", "--no-incremental"], root)
        assert replayed.returncode == 0, (
            f"{name}: QZI execution failed ({replayed.returncode})\n{replayed.stdout}"
        )
        coff = root / "retention.obj"
        compiled = run([
            qz, "build", artifact, "-c", "--target", "x86_64-windows", "-o", coff,
            "--no-incremental", "--no-progress",
        ], root)
        assert compiled.returncode == 0, f"{name}: COFF emission failed\n{compiled.stdout}"
        assert coff.read_bytes()[:2] == b"\x64\x86", f"{name}: expected x86-64 COFF"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("qz", type=Path, help="compiler executable")
    qz = parser.parse_args().qz.resolve(strict=True)
    failures = []
    with tempfile.TemporaryDirectory(prefix="quazi-result-retention-") as temporary:
        root = Path(temporary)
        for reject, cases in [(True, NEGATIVE_CASES), (False, POSITIVE_CASES)]:
            for name, source in cases.items():
                try:
                    check_case(qz, root / name, name, source, reject=reject)
                    print(f"PASS {name}: {'S10 rejection' if reject else 'native execution'}")
                except (AssertionError, subprocess.TimeoutExpired) as error:
                    failures.append(str(error))
                    print(f"FAIL {name}")
    if failures:
        raise SystemExit("\n\n".join(failures))
    print(f"PASS {len(NEGATIVE_CASES)} unsafe cases and {len(POSITIVE_CASES)} safe cases")


if __name__ == "__main__":
    main()
