#!/usr/bin/env python3
"""Check module-qualified spellings of imported types: `lib.Type.function()`
calls infer generic impl parameters, enum variants work after a symbol import
(`import lib.Shape;` then `Shape.Circle`), and `lib.Shape.Circle` is rejected
with a diagnostic naming that remedy instead of a misleading export error.
A private function cannot be called through a module path from another file."""

import argparse
import subprocess
import tempfile
from pathlib import Path

LIB = """pub enum Shape { Empty, Circle(i32), }
pub fn area(shape: Shape) i32 { ret match shape { Empty => 0, Circle(r) => r * 3 }; }
pub struct Cell[T] { value: T, }
impl Cell[T] {
    pub fn new(value: T) Cell[T] { ret Cell.with(value); }
    pub fn with(value: T) Cell[T] { ret Cell { value: value }; }
    pub fn get(self: &Cell[T]) &T { ret &self.value; }
}
fn hidden() i32 { ret 7; }
pub fn shown() i32 { ret lib.hidden() - 7; }
"""

VALID = """import lib;
import lib.Shape;
fn main() i32 {
    const empty: lib.Shape = Shape.Empty;
    const circle: Shape = Shape.Circle(4);
    const cell = lib.Cell.new(30);
    const seen: i32 = match &empty { Shape.Empty => 1, Shape.Circle(_) => 0 };
    ret lib.area(circle) + lib.area(empty) + *cell.get() + seen + lib.shown();
}
"""

INVALID = [
    ("import lib;\nfn main() i32 { const s: lib.Shape = lib.Shape.Empty; ret lib.area(s); }\n",
     "enum variants cannot be reached through a module path yet: add `import lib.Shape;` and write `Shape.Empty`"),
    ("import lib;\nfn main() i32 { ret lib.area(lib.Shape.Circle(2)); }\n",
     "add `import lib.Shape;` and write `Shape.Circle`"),
    ("import lib;\nfn main() i32 { ret lib.missing(); }\n",
     "'missing' is not exported by 'lib'"),
    # A private function stays private through a module path.
    ("import lib;\nfn main() i32 { ret lib.hidden(); }\n",
     "function `hidden` is private to its module"),
]


def project(root: Path, name: str, source: str) -> Path:
    path = root / name
    (path / "src").mkdir(parents=True)
    (path / "quazi.toml").write_text(f'[package]\nname="{name}"\n[build]\nentry="src/main.qz"\n')
    (path / "src/main.qz").write_text(source)
    (path / "src/lib.qz").write_text(LIB)
    return path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--qz", type=Path, default=Path("target/debug/qz"))
    qz = str(parser.parse_args().qz.resolve())
    with tempfile.TemporaryDirectory(prefix="quazi-module-paths-") as directory:
        root = Path(directory)
        valid = project(root, "valid", VALID)
        result = subprocess.run([qz, "run", "--silent", "--no-incremental"], cwd=valid,
                                text=True, capture_output=True, timeout=600)
        assert result.returncode == 43, (result.returncode, result.stdout + result.stderr)
        for index, (source, message) in enumerate(INVALID):
            invalid = project(root, f"invalid{index}", source)
            result = subprocess.run([qz, "check"], cwd=invalid,
                                    text=True, capture_output=True, timeout=600)
            output = result.stdout + result.stderr
            assert result.returncode != 0 and message in output, (source, output)
    print(f"PASS module paths: qualified calls and imported variants run; {len(INVALID)} invalid uses diagnosed")


if __name__ == "__main__":
    main()
