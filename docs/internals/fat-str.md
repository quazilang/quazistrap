# Fat `str` (D-030) — implementation plan

Audience: compiler developers.

Status: stage 1 landed for concrete plain structs (fixed-array fields,
2026-11-27); stage 2 landed (variadics with element stride; inline enum payloads;
multi-slot fn-value calls and raw-pointer moves); stages 3-7 open. Decision: [D-030](../decisions/fat-str.md)
(`str` is `{ptr, len}`, no terminating NUL).

## Measured starting point (2026-11-27)

Switching only `runtime_value_layout(Str)` to the two-slot `Slice` layout
fails 68 of 1195 compiler tests, all at existing one-slot gates:

| Gate (diagnostic) | Where str hits it |
|---|---|
| "cannot store `str` through the current one-slot aggregate representation" | struct fields: `String.data`, `PanicInfo.message`, `PanicInfo.file` |
| "cannot be passed by value in variadic parameter ... one-slot internal ABI" | `println`, `fmt.format` and every `...args: str` |
| "enum payloads use the one-slot representation" | `Result[T, str]`, `Option[str]` |

Fixed arrays, locals, parameters, results and contiguous containers
(`Array[T]` element stride) already handle multi-slot values.

## Stages

Each stage ends with `cargo test`, std `qz test`, every `scripts/check_*.py`
(Wine and NetBSD columns where available) passing. Run the scripts and std
with the release compiler (`cargo build --release`, `--qz
target/release/qz`); the debug compiler is several times slower.

1. Multi-slot struct fields: layout-driven field offsets and multi-slot
   field load/store/drop/move. Test vehicle: slice (`[T]`) fields, which hit
   the same gate today; `str` stays one slot.
2. Multi-slot enum payloads (layout-driven payload offsets, `match`
   bindings, drop glue) and variadic parameters (internal ABI).
3. Multi-slot captures in closures, `dyn` values, generic `T` storage.
4. Flip `str` to `{ptr, len}`: literals carry their length (constant pool
   keeps a trailing NUL only as an implementation detail), `StrLen` reads
   slot 1, `str_from_ptr` measures once, `String.as_str` builds the pair
   (`String` keeps `data`/`len`/`cap`; `data` becomes `*u8!`), formatting
   and concatenation produce lengths, `byte_at` bounds checks become O(1).
5. FFI: `@api` parameters of type `str` get a temporary NUL-terminated
   copy (or require `CStr`); `@api` results of `char*` are measured;
   `@repr(C)` `str` fields stay one `char*`; audit every
   `core.str_as_ptr`/`core.strlen` use in std that assumed a terminator.
6. QZI format and QZC cache bump, JIT, Windows COFF/PE, NetBSD.
7. Zero-copy `str` slicing; remove std/quazifetch workarounds for O(n)
   `bytes_len` (e.g. `raw_byte`).

## Stage 3 inventory (measured 2026-11-28)

After stages 1-2, flipping only the layout fails 19 compiler tests (one
remaining gate: generic specializations passing variadic `str`,
typecheck `record_fn_value_layout`). The runtime work is in the places
that treat `str` as a NUL-terminated `char*`:

| Site | Today | With `{ptr, len}` |
|---|---|---|
| string literal (`MovConst` of `ConstPoolEntry::Str`) | address | address + constant length |
| `StrLen` / `quazi.strlen` on a `str` | scans for NUL | `Mov` of slot 1; the opcode stays for C pointers |
| `StrAsStr` (`quazi.str.from_ptr`) | moves the pointer | pointer + one `strlen` |
| `StrConcat` (`quazi.str_concat`) | strlen of both | lengths from slot 1 |
| `PrimToStr` | static NUL buffer | buffer + its length |
| `StrToInt` / `StrToFloat` | parse to NUL | parse `len` bytes |
| `quazi.str.cmp`, `byte_at`, `rune_len`, `as_ptr` | NUL scans | lengths; `as_ptr` = slot 0 |
| `core.write(fd, s)` and the print path | `strlen` | slot 1 |
| `@api` parameters of type `str` | pointer passed | temporary NUL copy (or `CStr`) |
| `@api` results of type `str` | pointer | pointer + `strlen` |
| startup stubs building `main(args: Array[str])`, env access | 8-byte elements | 16-byte elements + `strlen` |
| `String.data: str` and the checked String ABI | data at offset 0 | keep `data: str`; `as_str` builds `{data.ptr, len}` from `String.len` (the field's own length may be stale after in-place appends) |
| panic messages and file names | constants | constants with lengths |
| std raw uses (`core.str_as_ptr(s)` to C, `core.strlen(s)`) | rely on NUL | audit each (fs, os, netbsd, process, net, io, tls) |

