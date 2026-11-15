#!/usr/bin/env python3
"""Check module-level constants (В20А): use before declaration, other
modules (`limits.MAX`, `import limits.MAX`), private access rejected, and a
QZI library whose `pub const` values a QZI-only consumer reads."""

import argparse
import subprocess
import tempfile
from pathlib import Path


def run(command, cwd, ok=True):
    result = subprocess.run(command, cwd=cwd, text=True, capture_output=True, timeout=600)
    if ok:
        assert result.returncode == 0, f"{command!r}: {result.stdout}{result.stderr}"
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--qz", type=Path, default=Path("target/debug/qz"))
    qz = str(parser.parse_args().qz.resolve())
    with tempfile.TemporaryDirectory(prefix="quazi-consts-") as directory:
        root = Path(directory)
        app = root / "app"
        (app / "src").mkdir(parents=True)
        (app / "quazi.toml").write_text('[package]\nname="app"\n[build]\nentry="src/main.qz"\n')
        (app / "src/limits.qz").write_text(
            "pub const MAX: i32 = 100;\nconst PRIVATE: i32 = 7;\npub fn twice() i32 { ret MAX * 2 + PRIVATE; }\n")
        (app / "src/main.qz").write_text(
            "import std.io.println;\nimport limits;\nimport limits.MAX;\n"
            "fn area() i64 { ret WIDTH * HEIGHT; }\nconst WIDTH: i64 = 40;\nconst HEIGHT: i64 = WIDTH / 2;\n"
            "fn main() i32 {\n    println(\"{} {} {} {}\", limits.MAX, MAX, limits.twice(), area());\n    ret 0;\n}\n")
        output = run([qz, "run", "--silent", "--no-incremental"], app).stdout
        assert output == "100 100 207 800\n", output
        (app / "src/main.qz").write_text("import limits;\nfn main() i32 { ret limits.PRIVATE; }\n")
        private = run([qz, "check"], app, ok=False)
        assert private.returncode != 0 and "private to its module" in private.stdout + private.stderr

        library = root / "constlib"
        (library / "src").mkdir(parents=True)
        (library / "quazi.toml").write_text('[package]\nname="constlib"\n\n[lib]\npath="src/lib.qz"\n')
        (library / "src/lib.qz").write_text(
            'pub const LIMIT: i32 = 42;\npub const NAME: str = "lib";\npub fn limit() i32 { ret LIMIT; }\n')
        run([qz, "build", "--silent", "--no-incremental", "-o", str(root / "constlib.qzi")], library)
        consumer = root / "consumer"
        (consumer / "src").mkdir(parents=True)
        (consumer / "quazi.toml").write_text(
            '[package]\nname="consumer"\n\n[dependencies]\n'
            f'constlib = {{ path = "{root / "constlib.qzi"}", type = "qzi" }}\n\n[build]\nentry="src/main.qz"\n')
        (consumer / "src/main.qz").write_text(
            "import std.io.println;\nimport constlib;\n"
            "fn main() i32 {\n    println(\"{} {} {}\", constlib.LIMIT, constlib.NAME, constlib.limit());\n    ret 0;\n}\n")
        output = run([qz, "run", "--silent", "--no-incremental"], consumer).stdout
        assert output == "42 lib 42\n", output
    print("PASS module constants: forward use, cross-module and imported access, private rejected, QZI library export")


if __name__ == "__main__":
    main()
