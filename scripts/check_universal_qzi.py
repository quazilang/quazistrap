#!/usr/bin/env python3
"""Check the unified multi-target QZI (format 16, qzifix.txt):

A. one project is ONE artifact: shared function bodies are stored once (for
   every target), the @cfg(target_os) alternatives once per target;
B. that file compiles natively for Linux, Windows and NetBSD, each binary
   running its own branches (Linux runs; Windows under Wine with --wine;
   NetBSD contains no Linux `syscall` and links only libc);
C. native generation needs no source files;
D. shared code calls target-specific code and passes owned values across;
   a program without a Windows implementation yields a QZI without that
   target and a clear error when compiling it for Windows;
E. `qz pack` keeps independent QZI artifacts whole and distinguishable.
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
fn shared_label(count: i32) String {
    var label = String.from("shared-");
    label.push_str(count.to_string().as_str());
    label
}
fn main() i32 {
    const label = shared_label(7);
    const name = system(String.from("os="));
    println("{} {}", label, name);
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
        run([qz, "build", "--no-incremental", "--silent", "-i", "-o", str(root / "app.qzi")], work)

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

        # C: no sources.
        shutil.rmtree(work)
        out = root / "out"
        out.mkdir()
        # B: one file, three targets.
        for target in targets:
            run([qz, "build", str(root / "app.qzi"), "--silent", "--target", f"x86_64-{target}",
                 "-o", str(out / target)], out)
        assert run([str(out / "linux")], out).stdout == b"shared-7 os=linux\n"
        if args.wine:
            env = dict(os.environ, WINEDEBUG="-all")
            exe = out / "windows.exe" if (out / "windows.exe").exists() else out / "windows"
            output = run([str(args.wine), str(exe)], out, env=env).stdout.replace(b"\r", b"")
            assert output == b"shared-7 os=windows\n", output
        netbsd = (out / "netbsd").read_bytes()
        disassembly = run(["objdump", "-D", "-b", "binary", "-m", "i386:x86-64", str(out / "netbsd")],
                          out).stdout
        assert b"syscall" not in disassembly, "Linux syscall in the NetBSD binary"
        assert b"libc.so.12" in netbsd and b"os=windows" not in netbsd and b"linux" not in netbsd

        # D: a target without an implementation.
        project(work, LINUX_ONLY)
        built = run([qz, "build", "--no-incremental", "-i", "-o", str(root / "partial.qzi")], work)
        assert b"no variant for windows, netbsd" in built.stderr, built.stderr
        failed = run([qz, "build", str(root / "partial.qzi"), "--target", "x86_64-windows",
                      "-o", str(out / "partial")], out, ok=False)
        assert failed.returncode != 0 and b"has no `windows` target" in failed.stderr, failed.stderr

        # E: packaging independent artifacts.
        run([qz, "pack", str(root / "app.qzi"), str(root / "partial.qzi"), "-o", str(root / "bundle.qzi")], root)
        listing = run([qz, "pack", "--list", str(root / "bundle.qzi")], root).stdout.decode()
        assert listing.splitlines()[0].startswith("app\t") and listing.splitlines()[1].startswith("partial\t"), listing
        run([qz, "pack", "--extract", "app", str(root / "bundle.qzi"), "-o", str(root / "extracted.qzi")], root)
        assert (root / "extracted.qzi").read_bytes() == data, "a packaged artifact changed"
        direct = run([qz, "build", str(root / "bundle.qzi"), "-o", str(out / "bundle")], out, ok=False)
        assert direct.returncode != 0 and b"QZI package" in direct.stderr, direct.stderr
    print("PASS unified QZI: one artifact with shared bodies once and per-target alternatives; "
          f"linux/windows/netbsd natives without sources{' (windows under Wine)' if args.wine else ''}; "
          "missing target diagnosed; qz pack keeps artifacts whole")


if __name__ == "__main__":
    main()
