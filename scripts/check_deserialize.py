#!/usr/bin/env python3
"""Check derived JSON decoding (D-012 step 4, maintainer answer В19В):
@derive(Deserialize) generates Type.from_json(source); an unknown key is
UnknownField, a repeated key DuplicateField, a missing required field
MissingField, a value of the wrong JSON type TypeMismatch, and an Option
field is None when missing or null. Serialize output decodes back. The wider
matrix (every integer width, floats, nested structs, Option and Array fields)
round-trips, range and element errors propagate, and repeated decoding of a
large struct does not leak."""

import argparse
import os
import subprocess
import tempfile
from pathlib import Path

SOURCE = 'import std.io.println;\nimport std.codec;\n\n@derive(Serialize, Deserialize)\nstruct Request {\n    enabled: bool @json(name="active"),\n    offset: i64,\n    label: String,\n}\n\n@derive(Deserialize)\nstruct Profile {\n    name: String,\n    age: Option[i64],\n    nick: Option[String],\n}\n\nfn describe(result: Result[Profile, codec.DecodeError]) void {\n    match result {\n        Ok(p) => println("ok {} {} {}", p.name, p.age.is_some(), p.nick.is_some()),\n        Err(e) => println("err {}", e.message()),\n    };\n}\n\nfn main() i32 {\n    const request = Request { enabled: true, offset: -42, label: String.from("Ada") };\n    const encoded: String = request.to_json().unwrap();\n    println("{}", encoded);\n    const decoded = Request.from_json(encoded.as_str()).unwrap();\n    println("{} {} {}", decoded.enabled, decoded.offset, decoded.label);\n    describe(Profile.from_json("{{\\"name\\": \\"Bo\\", \\"age\\": 30}}"));\n    describe(Profile.from_json("{{\\"name\\": \\"Cy\\", \\"nick\\": null}}"));\n    describe(Profile.from_json("{{\\"age\\": 3}}"));\n    describe(Profile.from_json("{{\\"name\\": \\"D\\", \\"extra\\": 1}}"));\n    describe(Profile.from_json("{{\\"name\\": \\"E\\", \\"name\\": \\"F\\"}}"));\n    describe(Profile.from_json("{{\\"name\\": 5}}"));\n    ret 0;\n}\n'

EXPECTED = """{"active":true,"offset":-42,"label":"Ada"}
true -42 Ada
ok Bo true false
ok Cy false false
err JSON object is missing a required field
err JSON object contains an unknown field
err JSON object contains a duplicate field
err JSON value has an incompatible type
"""

MATRIX = r"""import std.io.println;
import std.codec;

@derive(Serialize, Deserialize)
struct Point { x: i32, y: f64, }

@derive(Serialize, Deserialize)
struct Shape {
    name: String,
    small: u8,
    big: u64,
    wide: isize,
    ratio: f32,
    origin: Point,
    tags: Array[String],
    counts: Array[i16],
    path: Array[Point],
    label: Option[String],
    weight: Option[f32],
    anchor: Option[Point],
    flag: bool,
}

fn main() i32 {
    const source = "{{\"name\": \"tri\", \"small\": 255, \"big\": 18446744073709551615, \"wide\": -9, \"ratio\": 0.5, \"origin\": {{\"x\": -3, \"y\": 1.5}}, \"tags\": [\"a\", \"b\\n\"], \"counts\": [1, -2], \"path\": [{{\"x\": 1, \"y\": 2.25}}, {{\"x\": 4, \"y\": -1}}], \"weight\": null, \"anchor\": {{\"x\": 9, \"y\": 0.25}}, \"flag\": true}}";
    const shape: Shape = Shape.from_json(source).unwrap();
    println("{} {} {} {}", shape.small, shape.origin.x, shape.tags.len(), shape.path.len());
    const encoded: String = shape.to_json().unwrap();
    println("{}", encoded);
    const again: Shape = Shape.from_json(encoded.as_str()).unwrap();
    const twice: String = again.to_json().unwrap();
    println("{}", twice.as_str() == encoded.as_str());
    println("{}", Point.from_json("{{\"x\": 2147483648, \"y\": 0}}").unwrap_err().message());
    println("{}", Shape.from_json("{{\"name\": \"x\", \"small\": 1, \"big\": 1, \"wide\": 1, \"ratio\": 1, \"origin\": {{\"x\": 1, \"y\": 1}}, \"tags\": [1], \"counts\": [], \"path\": [], \"flag\": false}}").unwrap_err().message());
    println("{}", Shape.from_json("{{\"name\": \"x\", \"small\": 1, \"big\": 1, \"wide\": 1, \"ratio\": 1, \"origin\": {{\"x\": 1}}, \"tags\": [], \"counts\": [], \"path\": [], \"flag\": false}}").unwrap_err().message());
    var failures: i32 = 0;
    var i: i32 = 0;
    for (i < 2000) {
        const copy: Shape = Shape.from_json(source).unwrap();
        const text: String = copy.to_json().unwrap();
        if (text.bytes_len() != encoded.bytes_len()) { failures += 1; }
        if (Shape.from_json("{{\"name\": \"x\", \"small\": 1, \"big\": 1, \"wide\": 1, \"ratio\": 1, \"origin\": {{\"x\": 1, \"y\": 1}}, \"tags\": [\"a\"], \"counts\": [1], \"path\": [], \"flag\": 3}}").is_ok()) { failures += 1; }
        i++;
    }
    println("failures {failures}");
    ret 0;
}
"""

MATRIX_EXPECTED = """255 -3 2 2
{"name":"tri","small":255,"big":18446744073709551615,"wide":-9,"ratio":0.5,"origin":{"x":-3,"y":1.5},"tags":["a","b\\n"],"counts":[1,-2],"path":[{"x":1,"y":2.25},{"x":4,"y":-1}],"label":null,"weight":null,"anchor":{"x":9,"y":0.25},"flag":true}
true
JSON value is not valid for the destination type
JSON value has an incompatible type
JSON object is missing a required field
failures 0
"""

SERDE = r"""import std.io.println;
import std.codec;
import std.codec.DecodeError;
@serde("json")
struct Inner { x: i32, }
@serde("json")
struct User {
    id: usize,
    name: String,
    @serde("user_age")
    age: u8,
    level: u8 @serde("json:lvl"),
    inner: Inner,
}
trait Make { @associated fn make(n: i32) Self; }
impl Make for Inner { fn make(n: i32) Inner { ret Inner { x: n }; } }
fn build[T: Make](n: i32) T { ret T.make(n); }
fn parse[T: codec.Deserialize](text: str) Result[T, DecodeError] { ret T.from_json(text); }
fn main() i32 {
    var doc = codec.Document.parse("{{\"id\": 7, \"name\": \"Ada\", \"user_age\": 36, \"lvl\": 3, \"inner\": {{\"x\": 1}}}}");
    var user: User = doc.decode[User]().unwrap();
    user.age = user.age + 1;
    doc.encode(user).unwrap();
    println("{}", doc.text());
    const path = "/tmp/quazi-serde-document.json";
    doc.save_as(path).unwrap();
    const loaded = codec.Document.load(path).unwrap();
    const back: User = loaded.decode[User]().unwrap();
    println("{} {} {}", back.name, back.age, back.level);
    println("{}", codec.Document.parse("{{}}").save().is_err());
    const made: Inner = build[Inner](9);
    const parsed: Inner = parse[Inner]("{{\"x\": 4}}").unwrap();
    println("{} {}", made.x, parsed.x);
    ret 0;
}
"""
SERDE_EXPECTED = """{"id":7,"name":"Ada","user_age":37,"lvl":3,"inner":{"x":1}}
Ada 37 3
true
9 4
"""

SERDE_INVALID = """import std.codec;
@serde("yml")
struct A { x: i32 }
@serde("json")
struct B { @serde("toml:x") x: i32 }
struct C { @serde("x") x: i32 }
fn main() i32 { ret 0; }
"""

UNSUPPORTED = """import std.codec;
@derive(Deserialize)
struct Wide { grid: Array[Array[i64]] }
struct Plain { x: i64 }
@derive(Serialize)
struct Outer { inner: Plain }
fn main() i32 { ret 0; }
"""


def project(root, name, source):
    path = root / name
    (path / "src").mkdir(parents=True)
    (path / "quazi.toml").write_text(f'[package]\nname="{name}"\n[build]\nentry="src/main.qz"\n')
    (path / "src/main.qz").write_text(source)
    return path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--qz", type=Path, default=Path("target/debug/qz"))
    parser.add_argument("--wine", type=Path, help="also run the matrix as a Windows executable")
    args = parser.parse_args()
    qz = str(args.qz.resolve())
    with tempfile.TemporaryDirectory(prefix="quazi-deserialize-") as directory:
        root = Path(directory)
        valid = project(root, "valid", SOURCE)
        result = subprocess.run([qz, "run", "--silent", "--no-incremental"], cwd=valid,
                                text=True, capture_output=True, timeout=600)
        assert result.returncode == 0, result.stdout + result.stderr
        assert result.stdout == EXPECTED, result.stdout
        qzi = subprocess.run([qz, "build", "--silent", "--no-incremental", "-i", "-o",
                              str(root / "valid.qzi")], cwd=valid, text=True,
                             capture_output=True, timeout=600)
        assert qzi.returncode == 0, qzi.stdout + qzi.stderr
        portable = subprocess.run([qz, "build", str(root / "valid.qzi"), "--silent", "-o",
                                   str(root / "portable")], cwd=root, text=True,
                                  capture_output=True, timeout=600)
        assert portable.returncode == 0, portable.stdout + portable.stderr
        rerun = subprocess.run([str(root / "portable")], text=True, capture_output=True, timeout=60)
        assert rerun.stdout == EXPECTED, rerun.stdout
        matrix = project(root, "matrix", MATRIX)
        build = subprocess.run([qz, "build", "--silent", "--no-incremental", "-o",
                                str(root / "matrix-bin")], cwd=matrix, text=True,
                               capture_output=True, timeout=600)
        assert build.returncode == 0, build.stdout + build.stderr
        run = subprocess.run(["/usr/bin/env", "time", "-f", "%M", str(root / "matrix-bin")],
                             text=True, capture_output=True, timeout=300)
        assert run.returncode == 0, run.stdout + run.stderr
        assert run.stdout == MATRIX_EXPECTED, run.stdout
        assert int(run.stderr.strip().splitlines()[-1]) < 20000, run.stderr
        # The same matrix from a QZI artifact whose sources are gone.
        matrix_qzi = root / "matrix.qzi"
        build = subprocess.run([qz, "build", "--silent", "--no-incremental", "-i", "-o",
                                str(matrix_qzi)], cwd=matrix, text=True, capture_output=True,
                               timeout=600)
        assert build.returncode == 0, build.stdout + build.stderr
        (matrix / "src/main.qz").rename(matrix / "src/main.qz.moved")
        build = subprocess.run([qz, "build", str(matrix_qzi), "--silent", "-o",
                                str(root / "matrix-portable")], cwd=root, text=True,
                               capture_output=True, timeout=600)
        assert build.returncode == 0, build.stdout + build.stderr
        rerun = subprocess.run([str(root / "matrix-portable")], text=True, capture_output=True,
                               timeout=300)
        assert rerun.stdout == MATRIX_EXPECTED, rerun.stdout
        # Windows: a COFF object always; the executable under Wine when given.
        coff = subprocess.run([qz, "build", str(matrix_qzi), "--silent", "-c", "--target",
                               "x86_64-windows", "-o", str(root / "matrix.obj")], cwd=root,
                              text=True, capture_output=True, timeout=600)
        assert coff.returncode == 0, coff.stdout + coff.stderr
        assert (root / "matrix.obj").read_bytes()[:2] == b"\x64\x86"
        targets = "native, QZI-only, COFF"
        if args.wine:
            exe = root / "matrix.exe"
            build = subprocess.run([qz, "build", str(matrix_qzi), "--silent", "--target",
                                    "x86_64-windows", "-o", str(exe)], cwd=root, text=True,
                                   capture_output=True, timeout=600)
            assert build.returncode == 0, build.stdout + build.stderr
            env = dict(os.environ, WINEDEBUG="-all")
            run = subprocess.run([str(args.wine), str(exe)], text=True, capture_output=True,
                                 timeout=600, env=env)
            assert run.stdout.replace("\r\n", "\n") == MATRIX_EXPECTED, run.stdout + run.stderr
            targets += ", Windows (Wine)"
        serde = project(root, "serde", SERDE)
        result = subprocess.run([qz, "run", "--silent", "--no-incremental"], cwd=serde,
                                text=True, capture_output=True, timeout=600)
        assert result.returncode == 0, result.stdout + result.stderr
        assert result.stdout == SERDE_EXPECTED, result.stdout
        serde_invalid = project(root, "serde-invalid", SERDE_INVALID)
        result = subprocess.run([qz, "check"], cwd=serde_invalid, text=True,
                                capture_output=True, timeout=600)
        output = result.stdout + result.stderr
        assert result.returncode != 0, output
        for message in ['@serde format "yml" is not implemented yet', 'unknown @serde format "toml"',
                        "a field @serde rename needs @serde(\"format\") on its struct"]:
            assert message in output, (message, output)
        invalid = project(root, "unsupported", UNSUPPORTED)
        result = subprocess.run([qz, "check"], cwd=invalid, text=True, capture_output=True, timeout=600)
        output = result.stdout + result.stderr
        assert result.returncode != 0 and "Deserialize does not support field 'grid'" in output, output
        assert "Serialize field 'inner' has type Plain, which does not implement Serialize" in output, output
    print("PASS deserialize: round trip, key policy, Option fields, QZI without sources; @serde "
          "structs, renames, documents and @associated trait calls; wide numeric, "
          f"nested struct, Option and Array matrix without leaks ({targets}); unsupported fields rejected")


if __name__ == "__main__":
    main()
