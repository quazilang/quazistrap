#!/usr/bin/env python3
"""Check trait bounds on generic parameters (D-023, maintainer answer В22Б):
bounded functions and bounded generic types call trait methods on `T` for
each specialization; methods outside the bounds, methods on unbounded
parameters and instantiations with a type that lacks the trait are errors."""

import argparse
import subprocess
import tempfile
from pathlib import Path

PRELUDE = """import std.io.println;
trait Hashed { fn hash_code(self: &Self) u64; }
trait Named { fn label(self: &Self) str; }
struct Point { x: i32 }
impl Hashed for Point { fn hash_code(self: &Point) u64 { ret self.x as u64; } }
impl Named for Point { fn label(self: &Point) str { ret "pt"; } }
impl Hashed for i64 { fn hash_code(self: &i64) u64 { ret ((*self) as u64) * 3; } }
impl Named for i64 { fn label(self: &i64) str { ret "int"; } }
"""

VALID = PRELUDE + """struct Holder[T: Hashed + Named] { item: T }
impl Holder[T] {
    fn total(self: &Holder[T]) u64 { ret self.item.hash_code() + 100; }
}
fn describe[T: Hashed + Named](x: &T) u64 { println("{x.label()}"); ret x.hash_code() + 1; }
fn main() i32 {
    const p = Point { x: 41 };
    const h: Holder[Point] = Holder { item: Point { x: 2 } };
    const n: Holder[i64] = Holder { item: 5 };
    const v: i64 = 7;
    println("{describe[Point](&p)} {describe[i64](&v)} {h.total()} {n.total()}");
    ret 0;
}
"""

INVALID = [
    ("fn code[T: Hashed](x: &T) u64 { ret x.missing(); }\nfn main() i32 { ret 0; }\n",
     "no method `missing` on `T`: its bounds (Hashed) do not declare it"),
    ("fn code[T](x: &T) u64 { ret x.hash_code(); }\nfn main() i32 { ret 0; }\n",
     "add a trait bound such as `T: Trait`"),
    ("fn code[T: Hashed](x: &T) u64 { ret x.hash_code(); }\n"
     "fn main() i32 { const b = true; ret code[bool](&b) as i32; }\n",
     "`bool` does not implement `Hashed`, required by `T` of `code`"),
]


def project(root: Path, name: str, source: str) -> Path:
    path = root / name
    (path / "src").mkdir(parents=True)
    (path / "quazi.toml").write_text(f'[package]\nname="{name}"\n[build]\nentry="src/main.qz"\n')
    (path / "src/main.qz").write_text(source)
    return path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--qz", type=Path, default=Path("target/debug/qz"))
    qz = str(parser.parse_args().qz.resolve())
    with tempfile.TemporaryDirectory(prefix="quazi-bounds-") as directory:
        root = Path(directory)
        valid = project(root, "valid", VALID)
        result = subprocess.run([qz, "run", "--silent", "--no-incremental"], cwd=valid,
                                text=True, capture_output=True, timeout=600)
        assert result.returncode == 0, result.stdout + result.stderr
        assert result.stdout == "pt\nint\n42 22 102 115\n", result.stdout
        for index, (source, message) in enumerate(INVALID):
            invalid = project(root, f"invalid{index}", PRELUDE + source)
            result = subprocess.run([qz, "check"], cwd=invalid, text=True,
                                    capture_output=True, timeout=600)
            output = result.stdout + result.stderr
            assert result.returncode != 0 and message in output, (source, output)
    print(f"PASS trait bounds: bounded functions and types run; {len(INVALID)} invalid uses rejected")


if __name__ == "__main__":
    main()
