#!/usr/bin/env python3
"""Check universal QZI: one `qz build -i` artifact carries a variant for
every operating system, each with its own @cfg(target_os) implementation;
native builds from that one file (with the sources removed) pick the target's
variant only: the Linux binary prints the Linux branch, the Windows binary the
Windows branch (run under Wine when --wine is given), the NetBSD binary
equals a direct NetBSD build and contains no Linux `syscall`. A program that
compiles only for Linux yields a QZI whose other targets fail clearly."""

import argparse
import os
import shutil
import subprocess
import tempfile
from pathlib import Path

PROGRAM = """import std.io.println;
@cfg(target_os = "linux")
fn system() str { ret "linux"; }
@cfg(target_os = "windows")
fn system() str { ret "windows"; }
@cfg(target_os = "netbsd")
fn system() str { ret "netbsd"; }
fn main() i32 {
    var name: str = "unknown";
    @cfg(target_os = "linux") { name = "linux block"; }
    @cfg(target_os = "windows") { name = "windows block"; }
    @cfg(target_os = "netbsd") { name = "netbsd block"; }
    println("{} {}", system(), name);
    0
}
"""
LINUX_ONLY = """@cfg(target_os = "linux")
fn only() i32 { ret 0; }
fn main() i32 { ret only(); }
"""


def run(command, cwd, ok=True, env=None):
    result = subprocess.run(command, cwd=cwd, text=True, capture_output=True, timeout=600, env=env)
    if ok:
        assert result.returncode == 0, f"{command!r}: {result.stdout}{result.stderr}"
    return result


def project(root, source):
    shutil.rmtree(root, ignore_errors=True)
    (root / "src").mkdir(parents=True)
    (root / "quazi.toml").write_text('[package]\nname="universal"\n[build]\nentry="src/main.qz"\n')
    (root / "src/main.qz").write_text(source)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--qz", type=Path, default=Path("target/debug/qz"))
    parser.add_argument("--wine", type=Path)
    args = parser.parse_args()
    qz = str(args.qz.resolve())
    with tempfile.TemporaryDirectory(prefix="quazi-universal-") as directory:
        root = Path(directory)
        work = root / "work"
        project(work, PROGRAM)
        run([qz, "build", "--no-incremental", "--silent", "-i", "-o", str(root / "app.qzi")], work)
        direct = root / "direct-netbsd"
        run([qz, "build", "--no-incremental", "--silent", "--target", "x86_64-netbsd", "-o", str(direct)], work)
        shutil.rmtree(work)  # native builds must not need the sources
        out = root / "out"
        out.mkdir()
        for target in ("linux", "windows", "netbsd"):
            run([qz, "build", str(root / "app.qzi"), "--silent", "--target", f"x86_64-{target}",
                 "-o", str(out / target)], out)
        assert run([str(out / "linux")], out).stdout == "linux linux block\n"
        if args.wine:
            env = dict(os.environ, WINEDEBUG="-all")
            exe = out / "windows.exe" if (out / "windows.exe").exists() else out / "windows"
            output = run([str(args.wine), str(exe)], out, env=env).stdout.replace("\r", "")
            assert output == "windows windows block\n", output
        netbsd = out / "netbsd"
        assert netbsd.read_bytes() == direct.read_bytes(), "NetBSD from QZI differs from a direct build"
        disassembly = run(["objdump", "-d", str(netbsd)], out).stdout
        assert "syscall" not in disassembly, "Linux syscall in the NetBSD binary"
        assert "libc.so" in run(["readelf", "-d", str(netbsd)], out).stdout
        assert "NEEDED" not in run(["readelf", "-d", str(out / "linux")], out, ok=False).stdout

        project(work, LINUX_ONLY)
        built = run([qz, "build", "--no-incremental", "-i", "-o", str(root / "partial.qzi")], work)
        assert "no variant for windows, netbsd" in built.stderr, built.stderr
        failed = run([qz, "build", str(root / "partial.qzi"), "--target", "x86_64-windows",
                      "-o", str(out / "partial")], out, ok=False)
        assert failed.returncode != 0 and "no `windows` variant" in failed.stderr, failed.stderr
    print("PASS universal QZI: one artifact, linux/windows/netbsd variants selected at native build "
          f"(linux run{', windows under Wine' if args.wine else ''}, netbsd == direct build, no Linux syscalls); "
          "missing variants fail clearly")


if __name__ == "__main__":
    main()
