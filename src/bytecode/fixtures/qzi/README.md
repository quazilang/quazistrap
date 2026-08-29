# Historical QZI golden fixtures

Audience: language and tooling developers.

These `.qzi` files are immutable compatibility evidence. Each was produced by a
historical compiler built from the recorded commit — never by editing headers
of current artifacts. Do not regenerate them with a modified writer, do not
"refresh" them when the format changes, and do not fabricate new ones without
a real historical writer. The tests in `src/bytecode/chunk.rs` (`golden_*`)
lock the documented reader behavior against them.

## Inventory and provenance

| File | Writer commit | Date | sha256 |
|------|---------------|------|--------|
| `v2/main.qzi` | `f550025e05298dea851c48dfbd33b281a7e6fdcc` | 2026-07-28 | `81cfd416f5024522262e687937f2d05ec81bde1dd79246144404ae70e8b80f7e` |
| `v2/lea.qzi` | `f550025e05298dea851c48dfbd33b281a7e6fdcc` | 2026-07-28 | `29e2f594479ef27182f9ddd6f11a391e90a2f4b4fcfea5833e9014bbc3e350c7` |
| `v3/main.qzi` | `a5a7073a105bb8724f5f9e66403c46deb1fa3c93` | 2026-08-05 | `cf8cee1a18ac50eae53989775096594cc391b07ec5a0c9a8645ba921f7f323e8` |
| `v3/lea.qzi` | `a5a7073a105bb8724f5f9e66403c46deb1fa3c93` | 2026-08-05 | `b1ab843c55b4861ce337c5aeb47d871d91a62476f75aa3027f1c925cd2dfd606` |
| `v3/ffi.qzi` | `a5a7073a105bb8724f5f9e66403c46deb1fa3c93` | 2026-08-05 | `22eeec26c52fd5931a01e0fa3f128de316cb70cf2b6d423b6b539af321e99d5b` |
| `v4/main.qzi` | `abe480ad0d130a3d6d5b4dd2ac11480370090017` | 2026-08-06 | `939ed3d1625b3e57774b06f7f74e4c9a684fbe884d86d74d9c46e91400beed2d` |
| `v4/lea.qzi` | `abe480ad0d130a3d6d5b4dd2ac11480370090017` | 2026-08-06 | `0bd64814b2619a62403872883edce6393830474c61d2d3b23cd8350835542964` |
| `v4/ffi.qzi` | `abe480ad0d130a3d6d5b4dd2ac11480370090017` | 2026-08-06 | `fe71ca4e3d4ac935091409d6852cb35f1832cf2e16363e524328d7b2d951a14e` |
| `v5/main.qzi` | `8487aaf8595819a0a382c2d07a0320c8d82d0e4d` | 2026-08-06 | `cb2720e25ae553456d892266b493342ae3fa467da9f6d0bbf5ab33fffaea90e3` |
| `v5/lea.qzi` | `8487aaf8595819a0a382c2d07a0320c8d82d0e4d` | 2026-08-06 | `e5ff0067d72c1300ba5d70527e3539bb0d2a3f0ec4ed11b75f3f2ebeff9fa0bd` |
| `v5/ffi.qzi` | `8487aaf8595819a0a382c2d07a0320c8d82d0e4d` | 2026-08-06 | `ed09c4c559fb859c68e22a8183910be8a56fb5e91fbf7dd42da369cb4c6d6dde` |
| `v6/main.qzi` | `87d5d580cb2e6f39c678fe458b89ba9a06bcdd31` | 2026-08-12 | `7f4cc6b2d610b010942e67f196af39a81795754e94b93b25b0135563adfc5775` |
| `v6/lea.qzi` | `87d5d580cb2e6f39c678fe458b89ba9a06bcdd31` | 2026-08-12 | `4146bfcf315ea9960cfe7b48517db5b7426cd1bd568077f894aa3a54d7f77eff` |
| `v6/ffi.qzi` | `87d5d580cb2e6f39c678fe458b89ba9a06bcdd31` | 2026-08-12 | `bd5cb277c16e9732b22e8ae07b1d043d55d9b9c2bcb7f3ae963e23586f4cf280` |
| `v6/lib.qzi` | `87d5d580cb2e6f39c678fe458b89ba9a06bcdd31` | 2026-08-12 | `9ebafbd5e4201a9bb9c70097ce486c920a03ecc5b58b23f58dec6b21e1bf28c5` |

Generation procedure (2026-08-29): `git archive <commit>` into a scratch
directory, `cargo build --offline --bin qz` (toolchain 1.98.0, no manifest
edits), then `qz build <source>.qz -i -o <artifact>.qzi` with the era binary
(`--no-incremental` for v6). Every artifact was checked for the `\x00QZI`
magic and its version byte; repeated builds are byte-identical.

## What each fixture exercises

- `main.qzi` (v2-v6): era-safe feature tour — struct + impl constructor,
  enum with payload + `match`, generic identity monomorphization
  (`identity<i32>`), string constant, integer arithmetic, call chain. v4/v5
  also carry a `b"..."` byte-string constant. These decode successfully.
- `lea.qzi` (v2-v6): variadic calls and dynamic fixed-array indexing, which
  legacy writers emitted as `Lea` without address-taken register metadata.
  Every one must be rejected with "rebuild from source", proving the safety
  gate against real legacy bytecode.
- `ffi.qzi` (v3-v6): `@repr(C)` struct, `@export` function, `@api` import
  call; v5/v6 additionally declare an `@api("symbol") var` foreign global.
  These decode successfully.
- `lib.qzi` (v6 only): library-mode sectioned container with populated
  metadata (`goldenlib`/`0.1.0`/Library), a TOML public interface, and a
  (legitimately empty) relocation section — the v6 writer inlined and
  tree-shook every callee into `goldenlib.demo`. Non-empty relocations are
  covered by the v6 executable fixtures.

## Authentic historical behaviors recorded by these fixtures

- **v2 chunk headers have no flags byte.** v2 serialization is
  `name | param_count(u16) | reg_count(u8)`; intrinsic/variadic/export flags
  arrived with v3. The pre-fix reader assumed the v3+ header for v2 and
  misaligned (`unknown const tag ...`); it now reads the v2 layout and
  defaults the flags to zero. A v2 artifact therefore carries no
  intrinsic/variadic/export marks; intrinsic wrapper chunks lose their
  compilation-local symbol scope, which the QZI linker's chunk deduplication
  absorbs within a same-era input set. Mixing v2 and current artifacts that
  define the same intrinsic chunks instead fails explicitly with a
  conflicting-definitions link error, because chunk equivalence compares the
  flags v2 cannot carry.
- **v3 stores `@api` symbols as plain `Str` constants** referenced by
  `CallExt`; `ForeignSymbol` ABI metadata arrived with v4. The backend keeps a
  scalar-only legacy `CallExt` lowering for `Str` metadata, which matches
  v3's scalar/pointer-only FFI phase.
- **v3 does not persist `@export` symbols at all** (no chunk export field, no
  adapter chunk). A v3 artifact silently loses exported entry points; the
  information was never written and cannot be recovered by any reader.
- **v6 library builds require a prelude workaround.** The v6 writer rejected
  public generics (including the era prelude's own `pub trait Eq[T]`) before
  excluding the auto-injected prelude, so every v6 project-mode library build
  failed. `v6/lib.qzi` was generated with the era-native `QUAZI_PRELUDE_ROOT`
  override pointing at a minimal comment-only prelude; the writer binary was
  unmodified and the artifact is genuine v6 output (its bytecode section
  legitimately contains no prelude chunks). This was fixed after v6.
- **Historical reader note:** the v6-era reader itself could not read v2
  artifacts (it assumed the v3+ chunk header), confirming the header change
  above. v3-v5 artifacts read back fine under the v6 writer's own reader.

## Source programs

The sources below are the exact inputs. `main.qz` and `lea.qz` differ between
eras only in the header comment, the `qzi-golden-vN` string, and the noted
additions.

`main.qz` (v2; v3+ change the header comment and string, v4/v5 add the byte
string line before `ret`):

```quazi
// QZI v2 golden fixture (writer commit f550025, 2026-07-28).
struct Pair {
    first: i32,
    second: i32,
}

impl Pair {
    fn new(first: i32, second: i32) Pair {
        ret Pair { first: first, second: second };
    }

    fn sum(self: Pair) i32 {
        ret self.first + self.second;
    }
}

enum Value {
    Num(i32),
    Empty,
}

fn unwrap_or(v: Value, default: i32) i32 {
    ret match v {
        Value.Num(n) => n,
        Value.Empty => default,
    };
}

fn identity[T](x: T) T {
    ret x;
}

fn double(x: i32) i32 {
    ret x * 2;
}

fn inc(x: i32) i32 {
    ret x + 1;
}

fn main() i32 {
    var p = Pair.new(19, 23);
    var total = p.sum();

    var v = Value.Num(double(inc(total)));
    var n = unwrap_or(v, 0);
    var e = unwrap_or(Value.Empty, 7);

    var name: str = "qzi-golden-v2";

    var x = identity[i32](n - e);
    var y = (x % 10) + (p.second - p.first);

    // v4/v5 only: var magic = b"\x00QZI-golden-vN";
    ret y - 8;
}
```

`lea.qz` (all eras, header comment adjusted):

```quazi
// QZI v2 golden fixture (writer commit f550025, 2026-07-28).
fn total(...items: i32) i32 {
    var acc = 0;
    for it : items {
        acc += it;
    }
    ret acc;
}

fn main() i32 {
    var arr = [10, 20, 30, 40];
    var idx = total(1, 2, 3, 4) - 9;
    var picked = arr[idx];
    var more = total(5, 6);
    ret picked + more - 31;
}
```

`ffi.qz` (v3/v4; v5/v6 add the foreign global declaration and its use):

```quazi
// QZI v3 golden fixture (writer commit a5a7073, 2026-08-05).
@repr(C)
struct CPair {
    left: i32,
    right: i32,
}

@export("quazi_golden_sum")
pub fn sum_scalars(a: i32, b: i32) i32 {
    ret a + b;
}

@api("qzi_golden_ext")
unsafe fn ext_roundtrip(v: i32) i32;

// v5/v6 only:
// @api("qzi_golden_counter")
// var g_counter: i32;

fn pack(p: CPair) i32 {
    ret p.left * 256 + p.right;
}

fn main() i32 {
    var p = CPair { left: 1, right: 2 };
    var s = pack(p);
    var t = sum_scalars(s, 4);
    unsafe {
        t = ext_roundtrip(t);
        // v5/v6 only: t = t + g_counter;
    }
    ret t;
}
```

`v6` library project (`quazi.toml`: `[package] name = "goldenlib",
version = "0.1.0", type = "lib"`, default `src/lib.qz` entry):

```quazi
// QZI v6 library-mode golden fixture (writer commit 87d5d58, 2026-08-12).
pub fn add(a: i64, b: i64) i64 {
    ret a + b;
}

pub fn mul_add(a: i64, b: i64, c: i64) i64 {
    ret add(a, b) + c * a;
}

pub struct Point {
    x: i64,
    y: i64,
}

pub fn dist2(p: Point) i64 {
    ret p.x * p.x + p.y * p.y;
}

pub fn demo(n: i64) i64 {
    var p = Point { x: n, y: n + 1 };
    ret mul_add(dist2(p), 2, 3);
}
```
