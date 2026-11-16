#!/usr/bin/env python3
"""Check derived JSON decoding (D-012 step 4, maintainer answer В19В):
@derive(Deserialize) generates Type.from_json(source); an unknown key is
UnknownField, a repeated key DuplicateField, a missing required field
MissingField, a value of the wrong JSON type TypeMismatch, and an Option
field is None when missing or null. Serialize output decodes back."""

import argparse
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

UNSUPPORTED = """import std.codec;
@derive(Deserialize)
struct Wide { ratio: f64 }
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
    qz = str(parser.parse_args().qz.resolve())
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
        invalid = project(root, "unsupported", UNSUPPORTED)
        result = subprocess.run([qz, "check"], cwd=invalid, text=True, capture_output=True, timeout=600)
        output = result.stdout + result.stderr
        assert result.returncode != 0 and "Deserialize does not support field 'ratio'" in output, output
    print("PASS deserialize: round trip, key policy, Option fields, QZI without sources; unsupported field rejected")


if __name__ == "__main__":
    main()
