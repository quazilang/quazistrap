#!/usr/bin/env python3
"""Check trait bounds on generic parameters (D-023, maintainer answer В22Б):
bounded functions and bounded generic types call trait methods on `T` for
each specialization (`Some(x)` for an `Option[T]` parameter infers `T`, and
an owned bounded-call result is a fresh value); methods outside the bounds, methods on unbounded
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

VALID = PRELUDE + """import marks;
impl marks.Tag for Point { fn tag(self: &Point) u64 { ret 4; } }
fn tagged[T: marks.Tag](v: &T) u64 { ret v.tag(); }
struct Holder[T: Hashed + Named] { item: T }
impl Holder[T] {
    fn total(self: &Holder[T]) u64 { ret self.item.hash_code() + 100; }
}
struct Wrap[T: Hashed] { item: T }
impl Hashed for Wrap[T] { fn hash_code(self: &Wrap[T]) u64 { ret self.item.hash_code() + 1000; } }
struct Box2[T] { item: T }
struct Shelf[A] { a: A }
impl Shelf[A] {
    fn code[T: Hashed](self: &Shelf[A], x: &T) u64 { ret x.hash_code() + 1; }
    fn first[T](self: &Shelf[A], x: T) T { ret x; }
}
trait Text { fn text(self: Self) String; }
impl Text for Point { fn text(self: Point) String { ret String.from(self.label()); } }
fn first[T: Text](value: Option[T]) String {
    const none: String = String.from("none");
    ret match value { Some(inner) => inner.text(), None => none };
}
fn joined[T: Text](values: Array[T]) String {
    var out = String.new();
    for value : values { const piece: String = value.text(); out.push_str(piece.as_str()); }
    ret out;
}
fn describe[T: Hashed + Named](x: &T) u64 { println("{x.label()}"); ret x.hash_code() + 1; }
fn inner[T: Hashed](x: &T) u64 { ret x.hash_code(); }
fn outer[U: Hashed](x: &U) u64 { ret inner[U](x) + 1; }
fn main() i32 {
    const p = Point { x: 41 };
    const h: Holder[Point] = Holder { item: Point { x: 2 } };
    const n: Holder[i64] = Holder { item: 5 };
    const v: i64 = 7;
    println("{describe[Point](&p)} {describe[i64](&v)} {h.total()} {n.total()}");
    const w: Wrap[Wrap[Point]] = Wrap { item: Wrap { item: Point { x: 3 } } };
    const x: i64 = 9;
    const b: Box2[Box2[i64]] = Box2 { item: Box2 { item: x } };
    println("{outer(&p)} {w.hash_code()} {b.item.item} {tagged(&p)}");
    const plain: dyn Hashed = Point { x: 6 };
    const wrapped: Wrap[Point] = Wrap { item: Point { x: 7 } };
    const dynamic: dyn Hashed = wrapped;
    println("{plain.hash_code()} {dynamic.hash_code()}");
    const shelf: Shelf[i64] = Shelf { a: 1 };
    const flag: bool = shelf.first[bool](true);
    const three: i64 = shelf.first(3 as i64);
    println("{shelf.code(&p)} {flag} {three}");
    var points: Array[Point] = Array.new();
    points.push(Point { x: 1 });
    points.push(Point { x: 2 });
    const none: Option[Point] = None;
    println("{} {} {}", first(Some(Point { x: 5 })), first(none), joined(points));
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
    ("struct Only[T: Named] { item: T }\n"
     "fn main() i32 { const o: Only[i32] = Only { item: 1 }; ret o.item; }\n",
     "`i32` does not implement `Named`, required by `T` of `Only`"),
    ("enum Maybe[T: Named] { Some(T), Nothing }\n"
     "fn main() i32 { const m: Maybe[i32] = Maybe.Some(1); ret match &m { Maybe.Some(_) => 1, Maybe.Nothing => 0 }; }\n",
     "`i32` does not implement `Named`, required by `T` of `Maybe`"),
    ("fn code[T: Missing](x: &T) u64 { ret 1; }\nfn main() i32 { ret 0; }\n",
     "bound `T: Missing` of `code` does not name a trait"),
    ("struct Maker {}\nimpl Maker { fn make[T](self: &Maker) i32 { ret 0; } }\n"
     "fn main() i32 { const m = Maker {}; ret m.make(); }\n",
     "cannot infer type parameter `T` of `Maker.make`"),
    ("type Alias[T: Named] = Holder[T];\nfn main() i32 { ret 0; }\n",
     "a type alias cannot bound `T` by `Named` yet"),
    ("trait Visit { fn visit[V: Named](self: &Self, v: &V) u64; }\nfn main() i32 { ret 0; }\n",
     "a trait method cannot bound `V` by `Named` yet"),
]


def project(root: Path, name: str, source: str) -> Path:
    path = root / name
    (path / "src").mkdir(parents=True)
    (path / "quazi.toml").write_text(f'[package]\nname="{name}"\n[build]\nentry="src/main.qz"\n')
    (path / "src/main.qz").write_text(source)
    (path / "src/marks.qz").write_text("pub trait Tag { fn tag(self: &Self) u64; }\n")
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
        assert result.stdout == "pt\nint\n42 22 102 115\n42 2003 9 4\n6 1007\n42 true 3\npt none ptpt\n", result.stdout
        for index, (source, message) in enumerate(INVALID):
            invalid = project(root, f"invalid{index}", PRELUDE + source)
            result = subprocess.run([qz, "check"], cwd=invalid, text=True,
                                    capture_output=True, timeout=600)
            output = result.stdout + result.stderr
            assert result.returncode != 0 and message in output, (source, output)
    print(f"PASS trait bounds: bounded functions and types run; {len(INVALID)} invalid uses rejected")


if __name__ == "__main__":
    main()
