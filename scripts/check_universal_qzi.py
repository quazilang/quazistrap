#!/usr/bin/env python3
"""Check the unified multi-target QZI (format 16, qzifix.txt):

A. one project is ONE artifact: shared function bodies are stored once (for
   every target), the @cfg(target_os) alternatives once per target;
B. that file compiles natively for Linux, Windows and NetBSD, each binary
   running its own branches (Linux runs; Windows under Wine with --wine;
   NetBSD contains no Linux `syscall` and links only libc);
C. native generation needs no source files;
D. shared code calls target-specific code and passes owned values across;
   a program without a Windows implementation fails without publishing a
   partial QZI or replacing a previous artifact;
E. a project's dependencies are linked into its one QZI, which compiles
   without the dependency; a retired format 15 file is rejected.
"""

import argparse
import os
import shutil
import subprocess
import tempfile
from pathlib import Path

PROGRAM = """import std.io.println;
@cfg(target_os = "linux")
fn system(prefix: String) String { var text = prefix; text.push_str("linux"); text }
@cfg(target_os = "windows")
fn system(prefix: String) String { var text = prefix; text.push_str("windows"); text }
@cfg(target_os = "netbsd")
fn system(prefix: String) String { var text = prefix; text.push_str("netbsd"); text }
@cfg(target_abi = "sysv") type NativeWord = i32;
@cfg(target_abi = "win64") type NativeWord = [i32; 2];
fn native_width() usize { ret size_of[NativeWord](); }
fn shared_label(count: i32) String {
    var label = String.from("shared-");
    label.push_str(count.to_string().as_str());
    label
}
fn main() i32 {
    const label = shared_label(7);
    const name = system(String.from("os="));
    println("{} {} {}", label, name, native_width());
    0
}
"""
LINUX_ONLY = """@cfg(target_os = "linux")
fn only() i32 { ret 0; }
fn main() i32 { ret only(); }
"""


def run(command, cwd, ok=True, env=None):
    result = subprocess.run(command, cwd=cwd, capture_output=True, timeout=600, env=env)
    if ok:
        assert result.returncode == 0, (command, result.stdout, result.stderr)
    return result


def project(root, source, name="universal"):
    shutil.rmtree(root, ignore_errors=True)
    (root / "src").mkdir(parents=True)
    (root / "quazi.toml").write_text(f'[package]\nname="{name}"\n[build]\nentry="src/main.qz"\n')
    (root / "src/main.qz").write_text(source)


def unified_layout(data):
    """(targets, masks of every function body, body names) of a format-16 QZI."""
    assert data[:5] == b"\x00QZI\x10", "expected a unified QZI (format 16)"
    offset = 6
    targets = []
    for _ in range(data[5]):
        length = data[offset]
        targets.append(data[offset + 1:offset + 1 + length].decode())
        offset += 1 + length
    module_length = int.from_bytes(data[offset:offset + 4], "little")
    module = data[offset + 4:offset + 4 + module_length]
    offset += 4 + module_length
    count = int.from_bytes(data[offset:offset + 4], "little")
    masks = list(data[offset + 4:offset + 4 + count])
    return targets, masks, module


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--qz", type=Path, default=Path("target/debug/qz"))
    parser.add_argument("--wine", type=Path)
    args = parser.parse_args()
    qz = str(args.qz.resolve())
    with tempfile.TemporaryDirectory(prefix="quazi-unified-") as directory:
        root = Path(directory)
        work = root / "work"
        project(work, PROGRAM)
        built = run([qz, "build", "--no-incremental", "--no-color", "-i",
                     "-o", str(root / "app.qzi")], work)
        summary = [line for line in built.stderr.splitlines() if line.startswith(b"built ")]
        assert len(summary) == 1 and summary[0].endswith(b" QZI"), built.stderr

        # A: one artifact; shared bodies once, alternatives per target.
        data = (root / "app.qzi").read_bytes()
        targets, masks, module = unified_layout(data)
        assert targets == ["linux", "windows", "netbsd"], targets
        everyone = (1 << len(targets)) - 1
        assert module.count(b"shared_label") >= 1
        shared = sum(1 for mask in masks if mask == everyone)
        specific = {target: sum(1 for mask in masks if mask == 1 << index)
                    for index, target in enumerate(targets)}
        assert shared > 0 and all(specific.values()), (shared, specific)
        assert data.count(b"shared-") == 1, "a shared string constant is stored more than once"
        extra = work / "native.c"
        extra.write_text("invalid C must be ignored during QZI emission\n")
        direct = root / "explicit-source.qzi"
        run([qz, "build", str(work / "src/main.qz"), str(extra), "-i", "--silent",
             "-o", str(direct)], work)
        assert unified_layout(direct.read_bytes())[0] == targets

        # C: no sources.
        shutil.rmtree(work)
        out = root / "out"
        out.mkdir()
        # B: one file, three targets.
        for target in targets:
            run([qz, "build", str(root / "app.qzi"), "--silent", "--target", f"x86_64-{target}",
                 "-o", str(out / target)], out)
        output = run([str(out / "linux")], out).stdout
        assert output == b"shared-7 os=linux 8\n", output
        if args.wine:
            env = dict(os.environ, WINEDEBUG="-all")
            exe = out / "windows.exe" if (out / "windows.exe").exists() else out / "windows"
            output = run([str(args.wine), str(exe)], out, env=env).stdout.replace(b"\r", b"")
            assert output == b"shared-7 os=windows 16\n", output
        netbsd = (out / "netbsd").read_bytes()
        disassembly = run(["objdump", "-D", "-b", "binary", "-m", "i386:x86-64", str(out / "netbsd")],
                          out).stdout
        assert b"syscall" not in disassembly, "Linux syscall in the NetBSD binary"
        assert b"libc.so.12" in netbsd and b"os=windows" not in netbsd and b"linux" not in netbsd

        # D: a missing implementation fails both normal and quiet builds.
        project(work, LINUX_ONLY)
        partial = root / "partial.qzi"
        command = [qz, "build", "--no-incremental", "-i", "-o", str(partial)]
        failed = run(command, work, ok=False)
        assert failed.returncode != 0 and b"compilation failed for windows, netbsd" in failed.stderr, failed.stderr
        assert not partial.exists(), "failed build wrote a partial QZI"
        partial.write_bytes(data)
        failed = run([*command, "--silent"], work, ok=False)
        assert failed.returncode != 0 and b"compilation failed for windows, netbsd" in failed.stderr, failed.stderr
        assert partial.read_bytes() == data, "failed build replaced an existing QZI"
        project(work, '@cfg(target_os="linux") fn main() i32 { ret 0; }')
        manifest = work / "quazi.toml"
        manifest.write_text(manifest.read_text().replace("[package]\n", "[package]\nstd=false\n"))
        failed = run([*command, "--silent"], work, ok=False)
        assert failed.returncode != 0 and b"missing an entry for windows" in failed.stderr, failed.stderr
        assert partial.read_bytes() == data, "missing entry replaced an existing QZI"

        # E: a dependency is part of the project's single QZI.
        library = root / "mylib"
        (library / "src").mkdir(parents=True)
        (library / "quazi.toml").write_text('[package]\nname="mylib"\n\n[lib]\npath="src/lib.qz"\n')
        (library / "src/lib.qz").write_text("pub fn twice(x: i32) i32 { ret x * 2; }\n")
        consumer = root / "consumer"
        (consumer / "src").mkdir(parents=True)
        (consumer / "quazi.toml").write_text(
            '[package]\nname="consumer"\n\n[dependencies]\nmylib = { path = "../mylib" }\n\n'
            '[build]\nentry="src/main.qz"\n')
        (consumer / "src/main.qz").write_text("import mylib;\nfn main() i32 { ret mylib.twice(21); }\n")
        run([qz, "build", "--no-incremental", "-i", "-o", str(root / "consumer.qzi")], consumer)
        shutil.rmtree(library)
        shutil.rmtree(consumer)
        run([qz, "build", str(root / "consumer.qzi"), "-o", str(out / "consumer")], out)
        linked = subprocess.run([str(out / "consumer")], timeout=60)
        assert linked.returncode == 42, linked.returncode

        # Aliasing a compiled library preserves every conditional body.
        (library / "src").mkdir(parents=True)
        (library / "quazi.toml").write_text(
            '[package]\nname="target_lib"\nstd=false\n[lib]\npath="src/lib.qz"\n')
        (library / "src/lib.qz").write_text("".join(
            f'@cfg(target_os="{target}") pub fn value() i32 {{ ret {value}; }}\n'
            for target, value in [("linux", 11), ("windows", 22), ("netbsd", 33)]))
        dependency = root / "target_lib.qzi"
        run([qz, "build", "--lib", "--silent", "--no-incremental", "-o", str(dependency)], library)
        shutil.rmtree(library)
        (consumer / "src").mkdir(parents=True)
        (consumer / "quazi.toml").write_text(
            '[package]\nname="target_consumer"\nstd=false\n'
            '[dependencies]\nrenamed = { path = "../target_lib.qzi" }\n'
            '[build]\nentry="src/main.qz"\n')
        (consumer / "src/main.qz").write_text(
            "import renamed;\nfn main() i32 { ret renamed.value(); }\n")
        aliased = root / "aliased.qzi"
        run([qz, "build", "-i", "--silent", "--no-incremental", "-o", str(aliased)], consumer)
        _, alias_masks, _ = unified_layout(aliased.read_bytes())
        assert sorted(alias_masks) == [1, 2, 4, 7], alias_masks
        dependency.unlink()
        shutil.rmtree(consumer)
        run([qz, "build", str(aliased), "--silent", "-o", str(out / "aliased")], out)
        assert run([str(out / "aliased")], out, ok=False).returncode == 11

        # Preserve distinct public signatures even when the host interface
        # was cached first. Conditional source type checking needs the
        # matching interface before bytecode generation.
        (library / "src").mkdir(parents=True)
        (library / "quazi.toml").write_text(
            '[package]\nname="target_lib"\nstd=false\n[lib]\npath="src/lib.qz"\n')
        (library / "src/lib.qz").write_text(
            '@cfg(target_abi="sysv") pub fn value(value: i32) i32 { ret value; }\n'
            '@cfg(target_abi="win64") pub fn value(value: bool) i32 '
            '{ if (value) { ret 22; } ret 0; }\n')
        run([qz, "build", "--lib", "--silent", "--no-incremental", "-o", str(dependency)], library)
        shutil.rmtree(library)
        (consumer / "src").mkdir(parents=True)
        (consumer / "quazi.toml").write_text(
            '[package]\nname="target_consumer"\nstd=false\n'
            '[dependencies]\nrenamed = { path = "../target_lib.qzi" }\n'
            '[build]\nentry="src/main.qz"\n')
        (consumer / "src/main.qz").write_text(
            'import renamed;\n'
            '@cfg(target_abi="sysv") fn main() i32 { ret renamed.value(11); }\n'
            '@cfg(target_abi="win64") fn main() i32 { ret renamed.value(true); }\n')
        conditional = root / "conditional.qzi"
        run([qz, "build", "-i", "--silent", "--no-incremental", "-o", str(conditional)], consumer)
        dependency.unlink()
        shutil.rmtree(consumer)
        for target in targets:
            run([qz, "build", str(conditional), "--silent", "--target", f"x86_64-{target}",
                 "-o", str(out / f"conditional-{target}")], out)
        assert run([str(out / "conditional-linux")], out, ok=False).returncode == 11
        if args.wine:
            win = Path(str(out / "conditional-windows") + ".exe")
            if not win.exists():
                win = out / "conditional-windows"
            assert run([str(args.wine), str(win)], out, ok=False, env=env).returncode == 22

        (root / "retired.qzi").write_bytes(b"\x00QZI\x0f\x00\x00")
        retired = run([qz, "build", str(root / "retired.qzi"), "-o", str(out / "retired")], out, ok=False)
        assert retired.returncode != 0 and b"format 15" in retired.stderr, retired.stderr
    print("PASS unified QZI: one artifact with shared bodies once and per-target alternatives; "
          f"linux/windows/netbsd natives without sources{' (windows under Wine)' if args.wine else ''}; "
          "failed targets prevent publication; dependencies linked into the one QZI; format 15 rejected")


if __name__ == "__main__":
    main()
