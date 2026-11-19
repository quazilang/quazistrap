#!/usr/bin/env python3
"""Check std.io end-of-input: readln distinguishes an empty line (Ok("")) from
closed stdin (Err(ReadError.EndOfInput)), a final line without a newline is
still returned, and CR / CRLF line breaks are normalized."""

import argparse
import subprocess
import tempfile
from pathlib import Path

PROGRAM = r"""import std.io;

fn main() i32 {
    var lines: i64 = 0;
    for (true) {
        const line = io.readln();
        if (line.is_err()) { break; }
        const text = line.unwrap();
        io.println("[{}]", text.as_str());
        lines = lines + 1;
    }
    io.println("lines {lines}");
    0
}
"""


def check(qz, root):
    (root / "src").mkdir(parents=True)
    (root / "quazi.toml").write_text('[package]\nname="eof"\n[build]\nentry="src/main.qz"\n')
    (root / "src/main.qz").write_text(PROGRAM)
    binary = root / "native"
    build = subprocess.run([str(qz), "build", "--no-incremental", "--no-color", "--silent", "-o", str(binary)],
                           cwd=root, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=300)
    assert build.returncode == 0, build.stdout
    cases = [
        (b"a\n\nb", "[a]\n[]\n[b]\nlines 3\n"),
        (b"", "lines 0\n"),
        (b"x\n", "[x]\nlines 1\n"),
        # CRLF is one line break, a lone CR ends a line too (also across
        # reads: the LF after a CR that ended the previous line is skipped).
        (b"a\rb\r\nc\n", "[a]\n[b]\n[c]\nlines 3\n"),
        (b"x\r\n\r\ny", "[x]\n[]\n[y]\nlines 3\n"),
    ]
    for stdin, expected in cases:
        result = subprocess.run([str(binary)], input=stdin, stdout=subprocess.PIPE, timeout=30)
        assert result.stdout.decode() == expected, (stdin, result.stdout)
    print("PASS stdin EOF: empty lines, final unterminated line, EndOfInput, CR and CRLF breaks")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--qz", type=Path, default=Path("target/debug/qz"))
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="quazi-eof-") as directory:
        check(args.qz.resolve(), Path(directory))


if __name__ == "__main__":
    main()
