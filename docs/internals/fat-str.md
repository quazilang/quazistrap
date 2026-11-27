# Fat `str` (D-030) — implementation plan

Audience: compiler developers.

Status: planned; nothing landed yet. Decision: [D-030](../decisions/fat-str.md)
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
(Wine and NetBSD columns where available) passing.

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
