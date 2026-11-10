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


TEMPLATES = r"""import std.io;
fn main() void {
    io.println(`a {x} b`);
    io.println(`c {{y}}`);
    io.println("e {{z}}");
    io.println("f {{w}} {}", 1);
    ret;
}
"""


TRY = r"""import std.io;
import std.fs;

fn get(n: i64) Result[i64, str] {
    if (n % 2 == 0) { ret Err("even"); }
    Ok(n)
}

// `?` inside a placeholder leaves after a number was evaluated: neither the
// propagated Result nor any digit buffer may leak on either path.
fn show(n: i64) Result[usize, str] {
    const text = "a {n} {get(n)?} {n:x}";
    Ok(text.len())
}

fn mapped_pages() u64 {
    const result = fs.read_to_string("/proc/self/statm");
    if (result.is_err()) { ret 0; }
    const text: String = result.unwrap();
    const end: usize = text.as_str().find(" ").unwrap();
    const field = text[0:end];
    const parsed: Result[u64, ParseError] = field.parse[u64]();
    parsed.unwrap_or(0)
}

fn main() i32 {
    const before = mapped_pages();
    var failures: i64 = 0;
    for i : 0..400000 {
        if (show(i as i64).is_err()) { failures = failures + 1; }
    }
    // A 32-byte leak per call would map about 3000 more pages.
    const grown = mapped_pages() - before;
    io.println("failures {failures} small growth {grown < 500}");
    0
}
"""


# A `...values: str` without a template parameter is a plain typed variadic;
# it was compiled as a format wrapper (wrong arguments, out-of-bounds panic).
VARIADIC = r"""import std.io;

fn count(...xs: str) usize { xs.len() }
fn first(...xs: str) usize { xs[0].len() }

struct Bag { size: i32, }

// Variadic methods pack their tail like free functions (they received the
// raw arguments in place of the slice before).
impl Bag {
    fn count(self: &Bag, ...values: str) usize { values.len() }
    fn sum(self: &Bag, ...values: i32) i32 {
        var total = self.size;
        for value : values { total = total + value; }
        total
    }
}

fn main() i32 {
    const s: str = "hello";
    io.println("a {}", count("x", "y"));
    io.println("b {}", count(s, s));
    io.println("c {}", first("abc"));
    io.println("d {}", first(s));
    io.println("e {}", count());
    const bag = Bag { size: 1 };
    io.println("m {} {} {}", bag.count("x", "y"), bag.count(s), bag.count());
    io.println("n {} {}", bag.sum(7), bag.sum(2, 3, 4));
    0
}
"""


# Format methods, generic receivers and static associated calls with a
# variadic tail (each received unpacked or unformatted arguments before).
METHODS = r"""import std.io;

struct Logger { level: i32, }
struct Bag[T] { first: T, }
struct Kit { size: i32, }

impl Logger {
    fn show(self: &Logger, prefix: str, ...args: str) void {
        io.println("[{}] n={}", prefix, args.len());
    }
}

impl Bag[T] {
    fn sum(self: &Bag[T], ...values: i32) i32 {
        var total: i32 = 0;
        for value : values { total = total + value; }
        total
    }
}

impl Kit {
    fn make(...values: i32) i32 { values.len() as i32 }
    fn add(self: &Kit, ...values: i32) i32 {
        var total = self.size;
        for value : values { total = total + value; }
        total
    }
}

fn main() i32 {
    const x: i32 = 5;
    const l = Logger { level: 1 };
    l.show("p {x} {}", "q");
    const bag: Bag[i32] = Bag { first: 1 };
    io.println("generic {} {}", bag.sum(7), bag.sum(1, 2, 3));
    io.println("static {} {}", Kit.make(1, 2, 3), Kit.make());
    const kit = Kit { size: 10 };
    io.println("static receiver {}", Kit.add(&kit, 2, 3));
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
    case = project(root, "templates", TEMPLATES)
    assert invoke([str(qz), "run", *common], case) == "a {x} b\nc {{y}}\ne {z}\nf {w} 1\n"
    case = project(root, "closure", CLOSURE)
    assert invoke([str(qz), "run", *common], case) == "f 49\njoined 49\nh 3\n"
    case = project(root, "moved", MOVED)
    output = run([str(qz), "check"], case).stdout
    assert "use of moved value 's'" in output and "use of moved value 't'" in output, output
    case = project(root, "variadic", VARIADIC)
    output = invoke([str(qz), "run", *common], case)
    assert output == "a 2\nb 2\nc 3\nd 5\ne 0\nm 2 1 0\nn 8 10\n", output
    case = project(root, "methods", METHODS)
    output = invoke([str(qz), "run", *common], case)
    assert output == "[p 5 q] n=0\ngeneric 7 6\nstatic 3 0\nstatic receiver 15\n", output
    case = project(root, "try", TRY)
    output = invoke([str(qz), "run", *common], case)
    assert output == "failures 200000 small growth true\n", output
    print("PASS string interpolation: literals, escapes, raw, templates, verbatim values, native/QZI, 3 diagnostics, use-after-move in placeholders, closure captures, raw and placeholder-free templates, no leak when `?` leaves a placeholder, plain str variadics, variadic method forms")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--qz", type=Path, default=Path("target/debug/qz"))
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="quazi-interpolation-") as directory:
        check(args.qz.resolve(), Path(directory))


if __name__ == "__main__":
    main()
