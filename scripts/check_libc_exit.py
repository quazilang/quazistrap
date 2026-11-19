#!/usr/bin/env python3
"""Check that a program linked with libc flushes C stdio when it ends.

Without libc, Quazi ends the process with the raw `exit_group` syscall. With
libc (`-l c` or `[link] libc = true`), returning from `main` and
`core.exit` call libc `exit(3)` instead, so text that C code buffered (`puts`
into a pipe or file is fully buffered) is written and the status is kept."""

import argparse
import subprocess
import tempfile
from pathlib import Path

RETURN = r"""import std.core;
@api("puts")
unsafe fn puts(text: *u8) i32;
fn main() i32 {
    unsafe { puts(core.str_as_ptr("from main")); }
    ret 3;
}
"""
EXIT = r"""import std.core;
@api("puts")
unsafe fn puts(text: *u8) i32;
fn main() i32 {
    unsafe { puts(core.str_as_ptr("from exit")); }
    core.exit(4);
    ret 0;
}
"""


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--qz", type=Path, default=Path("target/debug/qz"))
    qz = str(parser.parse_args().qz.resolve())
    with tempfile.TemporaryDirectory(prefix="quazi-libc-exit-") as directory:
        root = Path(directory)
        for name, source, text, status in (("return", RETURN, "from main\n", 3),
                                           ("exit", EXIT, "from exit\n", 4)):
            (root / f"{name}.qz").write_text(source)
            binary = root / name
            build = subprocess.run([qz, "build", f"{name}.qz", "-l", "c", "--no-incremental",
                                    "--no-color", "--silent", "-o", str(binary)],
                                   cwd=root, text=True, capture_output=True, timeout=600)
            assert build.returncode == 0, build.stdout + build.stderr
            # stdout is a pipe here, so C stdio buffers it fully.
            result = subprocess.run([str(binary)], cwd=root, text=True, capture_output=True,
                                    timeout=60)
            assert (result.returncode, result.stdout) == (status, text), (
                name, result.returncode, result.stdout, result.stderr)
    print("PASS libc exit: buffered C stdio reaches a pipe after main returns and after core.exit")


if __name__ == "__main__":
    main()
