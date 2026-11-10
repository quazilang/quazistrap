# Numeric casts between integers and floats

Date: 2026-11-10

Audience: Quazi users and compiler developers.

The maintainers asked for complete `as` conversions (D-021, В15Г). `as` now
converts between the integer family and `f32`/`f64`:

- integer to float rounds to nearest-even directly at the target precision
  (`u64` values with the top bit set included);
- float to integer truncates toward zero and saturates: beyond the range the
  result is the target minimum or maximum, NaN is 0;
- `f64 as f32` rounds to nearest; `f32 as f64` is exact.

These are Rust's `as` rules; a cast is an explicit request, so it does not
panic. `f16` stays outside cross-family casts (S06).

Integer casts to a narrower or differently signed type now really truncate.
Before, the slot was left unchanged: `wide as u8` with `wide = 300` stayed 300
at run time and compared and printed as 300, while the documented result is
44. Constants fold with the same rules (`256 as u8` is 0).

Implementation: `emit_numeric_cast` in codegen sign- or zero-extends narrowed
integers with shifts or a mask, and emits compiler-private `Intrinsic` ids
51-63 (`bytecode::instruction::numeric_cast`) for the other conversions;
`backend/x86_64/numeric_cast.rs` lowers them with SSE2 (`cvtsi2sd`/`cvtsi2ss`,
`cvttsd2si` with range and NaN checks, `cvtsd2ss`). QZI readers accept the
new ids; older compilers reject artifacts that use them as unknown
intrinsics.

`as` binds tighter than unary minus (documented precedence), so `-1 as u16`
is `-(1 as u16)`; write `(-1) as u16`.

Known gap: arithmetic on integers narrower than 64 bits is not yet wrapped to
the type width (`65535 as u16 + 1` yields 65536), so a narrow value produced by
arithmetic may still be non-canonical before a cast.

Verification:

```bash
cargo test --offline numeric_cast
python3 scripts/check_numeric_casts.py --qz target/debug/qz
```
