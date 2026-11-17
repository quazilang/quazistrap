# Derived codecs cover numbers, nested structs, `Option` and `Array`

Date: 2026-11-17

Audience: Quazi users.

`@derive(Serialize)` accepted only `bool`, `i64` and `String` fields, and
`@derive(Deserialize)` added `Option` of those. Both now accept every integer
width, `f32`, `f64`, `String`, nested structs, and `Option` or `Array` of those
([field matrix](../api/codec.md#derived-field-matrix)). A nested struct must
implement `Serialize` (or define `from_json`), otherwise the field is rejected
with `S14`. Enums, generic structs, and nested containers such as
`Array[Array[i64]]` remain unsupported.

```quazi,ignore
@derive(Serialize, Deserialize)
struct Point { x: i32, y: f64, }

@derive(Serialize, Deserialize)
struct Shape { origin: Point, path: Array[Point], label: Option[String], }
```

Each field is decoded by its own generated private method
(`__quazi_json_field_<i>`), so no generated function grows with the struct.

Compiler changes found on the way:

- `Some(x)` passed to a generic `Option[T]` parameter infers `T` from `x`
  instead of failing with "Some payload: expected T".
- The owned result of a method called through a trait bound is no longer an
  unverified view source (each impl body is checked on its own), so
  `const s: String = value.to_json()?;` on a bounded `T` is accepted.
- `str.parse_f64` (and `String.parse_f64`/`parse_f32`) returns the nearest
  f64: it keeps the first 19 significant digits as an integer and scales it in
  double-double precision. It used to accumulate digit by digit, so `"0.3"`
  parsed to a different value than the literal `0.3`.
- A `let`-style binding takes its register (and drop guard) before its
  initializer, and a `?` failure path returns its scratch registers. Long
  functions with many `?` operators no longer exceed the 255 QZI registers.

Migration: none. `std.codec` adds `decode_i8`…`decode_usize`, `decode_f32`,
`optional_raw`, `array_items`, `optional_json` and `array_json`;
`decode_string` now returns a fresh copy without provenance in its input.
Float `to_json` emits the shortest decimal (at most 17 digits for f64, 9 for
f32) that decodes back to the same value, instead of six significant digits;
`decode_f64`/`decode_f32` reject numbers beyond their range (`InvalidValue`).

Verification: `scripts/check_float_format.py` (400 randomized parses
against Python), std `codec.floats_encode_to_the_shortest_round_trip_decimal`,
`scripts/check_deserialize.py` (round trip of a 13-field struct,
range/element errors, 2000 iterations under 20 MB peak memory),
`scripts/check_trait_bounds.py`, `scripts/check_try_propagation.py` (48 `?`
failure paths in one function), `cargo test`, std `qz test`.
