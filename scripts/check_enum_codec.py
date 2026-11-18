#!/usr/bin/env python3
"""Verify externally tagged JSON enum codecs and variant rename diagnostics."""

import argparse
import os
import subprocess
import tempfile
from pathlib import Path

from check_deserialize import project

IMPORTS = "import std.codec;\nimport std.io.println;\n"
SOURCE = IMPORTS + r"""
@derive(Serialize, Deserialize)
struct Point { x: i32, y: i32 }

@derive(Serialize, Deserialize)
enum Message {
    Empty,
    Text(String) @tag("label"),
    @tag("pair")
    Pair(i32, String),
    Count(Option[i32]),
    Values(Array[i32]),
    Position(Point),
    Braces @tag("braces{tag}"),
}

fn round_trip(source: str) void {
    const message = Message.from_json(source).unwrap();
    println("{}", message.to_json().unwrap().as_str());
}

fn invalid(source: str) void {
    const error: codec.DecodeError = Message.from_json(source).unwrap_err();
    const exact = match error { InvalidValue => true, _ => false };
    println("{}", exact);
}

fn main() i32 {
    round_trip("\"Empty\"");
    round_trip("{{\"label\":\"hello\"}}");
    round_trip("{{\"pair\":[7,\"ok\"]}}");
    round_trip("{{\"Count\":null}}");
    round_trip("{{\"Count\":12}}");
    round_trip("{{\"Values\":[2,3]}}");
    round_trip("{{\"Position\":{{\"x\":1,\"y\":2}}}}");
    round_trip("\"braces{{tag}}\"");
    invalid("\"Unknown\"");
    invalid("{{\"unknown\":7}}");
    invalid("{{\"pair\":[7]}}");
    invalid("{{\"Empty\":null}}");
    invalid("\"pair\"");
    invalid("{{\"label\":\"hello\",\"Count\":12}}");
    invalid("\"Empty\\u0000suffix\"");
    invalid("{{\"label\\u0000suffix\":\"hello\"}}");
    ret 0;
}
"""
EXPECTED = '''"Empty"
{"label":"hello"}
{"pair":[7,"ok"]}
{"Count":null}
{"Count":12}
{"Values":[2,3]}
{"Position":{"x":1,"y":2}}
"braces{tag}"
true
true
true
true
true
true
true
true
'''


def run(command, cwd):
    result = subprocess.run(command, cwd=cwd, capture_output=True, text=True, timeout=600)
    assert result.returncode == 0, (command, result.stdout, result.stderr)
    return result.stdout


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--qz", type=Path, default=Path("target/debug/qz"))
    parser.add_argument("--wine", type=Path)
    args = parser.parse_args()
    qz = str(args.qz.resolve())
    os.environ.setdefault("WINEDEBUG", "-all")
    with tempfile.TemporaryDirectory(prefix="quazi-enum-codec-") as directory:
        root = Path(directory)
        fixture = project(root, "enum-codec", SOURCE)
        assert run([qz, "run", "--silent", "--no-incremental"], fixture) == EXPECTED
        artifact = root / "message.qzi"
        run([qz, "build", "-i", "--silent", "--no-incremental", "-o", str(artifact)], fixture)
        (fixture / "src/main.qz").unlink()
        executable = root / "message"
        run([qz, "build", str(artifact), "--silent", "-o", str(executable)], root)
        assert run([str(executable)], root) == EXPECTED
        if args.wine:
            windows = root / "message.exe"
            run([qz, "build", str(artifact), "--silent", "--target", "x86_64-windows",
                 "-o", str(windows)], root)
            assert run([str(args.wine), str(windows)], root) == EXPECTED
        invalid = project(root, "invalid-enum-tag", IMPORTS + r"""
@derive(Serialize, Deserialize)
enum Bad { First @tag("Second"), Second }
fn main() void {}
""")
        checked = subprocess.run([qz, "check"], cwd=invalid, capture_output=True, text=True, timeout=60)
        assert checked.returncode != 0 and 'JSON tag `Second` is used by' in checked.stderr, checked.stderr
        # A library QZI exposing a derived enum links into a consumer: the
        # library's prelude bodies must match the consumer's (same destructors).
        library = root / "answers"
        (library / "src").mkdir(parents=True)
        (library / "quazi.toml").write_text('[package]\nname="answers"\n[lib]\npath="src/lib.qz"\n')
        (library / "src/lib.qz").write_text(
            "import std.codec;\n@derive(Serialize, Deserialize)\n"
            'pub enum Answer { Yes @tag("No"), No @tag("Yes"), Text(String) @tag("body") }\n'
        )
        run([qz, "build", "-i", "--silent", "--no-incremental", "-o", str(root / "answers.qzi")], library)
        consumer = project(root, "enum-consumer", """import std.codec;
import answers;
fn main() i32 {
    const a = answers.Answer.from_json("\\"No\\"").unwrap();
    ret match a { Yes => 0, No => 1, Text(_) => 2 };
}
""")
        with (consumer / "quazi.toml").open("a") as manifest:
            manifest.write('[dependencies]\nanswers={path="../answers.qzi"}\n')
        linked = subprocess.run([qz, "run", "--silent", "--no-incremental"], cwd=consumer,
                                capture_output=True, text=True, timeout=600)
        assert linked.returncode == 0, (linked.stdout, linked.stderr)
    print("PASS externally tagged enum codecs (native, QZI-only, library consumer, tag diagnostics)")


if __name__ == "__main__":
    main()
