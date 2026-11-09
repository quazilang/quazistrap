#!/usr/bin/env python3
"""Check D-020 string interpolation: `{expr}` and `{expr:spec}` in ordinary
literals, `{{`/`}}` escapes, raw strings staying verbatim, format-call
templates, verbatim printing of a non-literal value, and the brace
diagnostics, natively and from source-removed QZI."""

import argparse
import subprocess
import tempfile
from pathlib import Path

PROGRAM = r"""import std.io;

fn main() i32 {
    const x: i64 = 5;
    const name: str = "quazi";
    const greeting = "hello {name}, x = {x}, hex {x:X}";
    io.println(greeting.as_str());
    const braces: str = "json {{\"a\": 1}}";
    io.println(braces);
    const raw: str = `raw {x} {{y}}`;
    io.println(raw);
    io.println("template {} and {x} and {{lit}}", x + 1);
    io.println("len {}", braces.len());
    0
}
"""

EXPECTED = """hello quazi, x = 5, hex 5
json {"a": 1}
raw {x} {{y}}
template 6 and 5 and {lit}
len 13
"""

INVALID = r"""fn main() i32 {
    const a: str = "bad {";
    const b: str = "bad }";
    const c: str = "empty {}";
    0
}
"""


MOVED = r"""import std.io;

fn consume(text: String) i64 { text.len() as i64 }

fn main() i32 {
    const s: String = "secret".to_string();
    const n = consume(s);
    io.println("moved {s} {n}");
    const t: String = "other".to_string();
    const m = consume(t);
    const line = "again {t}";
    io.println(line.as_str());
    0
}
"""


CLOSURE = r"""import std.io;

fn square(n: i64) i64 { n * n }

fn main() i32 {
    const n: i64 = 7;
    const f: fn() i64 = || square(n);
    io.println("f {}", f());
    const g = qo square(n);
    io.println("joined {g.join().unwrap()}");
    var k: i64 = 3;
    const h: fn() usize = || "k={k}".len();
    io.println("h {}", h());
    0
}
"""


def run(command, root):
    return subprocess.run(command, cwd=root, text=True, stdout=subprocess.PIPE,
                          stderr=subprocess.STDOUT, timeout=300)


def invoke(command, root):
    result = run(command, root)
    assert result.returncode == 0, f"{command!r} returned {result.returncode}:\n{result.stdout}"
    return result.stdout


def project(root, name, source):
    case = root / name
    (case / "src").mkdir(parents=True)
    (case / "quazi.toml").write_text(f'[package]\nname="{name}"\n[build]\nentry="src/main.qz"\n')
    (case / "src/main.qz").write_text(source)
    return case


def check(qz, root):
    common = ["--no-incremental", "--no-color", "--no-unicode", "--silent"]
    case = project(root, "interpolation", PROGRAM)
    binary = case / "native"
    invoke([str(qz), "build", *common, "-o", str(binary)], case)
    assert invoke([str(binary)], case) == EXPECTED, invoke([str(binary)], case)
    artifact = case / "program.qzi"
    invoke([str(qz), "build", *common, "-i", "-o", str(artifact)], case)
    (case / "src/main.qz").unlink()
    portable = case / "portable"
    invoke([str(qz), "build", str(artifact), *common, "-o", str(portable)], root)
    assert invoke([str(portable)], case) == EXPECTED
    case = project(root, "invalid", INVALID)
    output = run([str(qz), "check"], case).stdout
    for message in ("unmatched `{`", "unmatched `}`", "`{}` has no argument outside a format call"):
        assert message in output, output
    case = project(root, "closure", CLOSURE)
    assert invoke([str(qz), "run", *common], case) == "f 49\njoined 49\nh 3\n"
    case = project(root, "moved", MOVED)
    output = run([str(qz), "check"], case).stdout
    assert "use of moved value 's'" in output and "use of moved value 't'" in output, output
    print("PASS string interpolation: literals, escapes, raw, templates, verbatim values, native/QZI, 3 diagnostics, use-after-move in placeholders, closure captures")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--qz", type=Path, default=Path("target/debug/qz"))
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="quazi-interpolation-") as directory:
        check(args.qz.resolve(), Path(directory))


if __name__ == "__main__":
    main()
