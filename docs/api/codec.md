# `std.codec`

`std.codec` defines the typed boundary above `std.json`. `Serialize` covers
booleans, every integer width, `f32`/`f64`, and text; typed decoding is exposed
through explicit `decode_*` functions. `@derive(Serialize)` and
`@derive(Deserialize)` support non-generic structs whose fields are scalars,
`String`, nested structs, and `Option` or `Array` of those (see
[the field matrix](#derived-field-matrix)).

## Encoding

`Serialize.to_json()` produces one owned JSON value token:

```quazi
import std.codec;

fn main() i32 {
    const enabled: String = true.to_json().unwrap();
    const offset: i64 = -42;
    const number: String = offset.to_json().unwrap();
    const label: String = "ready".to_json().unwrap();
    // `enabled` is `true`; `number` is `-42`; `label` is `"ready"`.
    ret 0;
}
```

The current implementations cover `bool`, `i8`–`i64`, `u8`–`u64`, `isize`,
`usize`, `f32`, `f64`, `str`, owned `String`, and `time.SystemTime`, which encodes as an RFC 3339 UTC JSON string such as
`"2000-02-29T12:34:56Z"`; `codec.decode_system_time(source)` decodes it
(numeric offsets are normalized to UTC, malformed text is `InvalidValue`).
Integers use their exact base-10 representation. A float uses the shortest
decimal (at most 17 significant digits for `f64`, 9 for `f32`) that decodes
back to the same value, in plain notation for moderate exponents and
scientific notation (`1.23e-7`) otherwise; a NaN or infinite float has no JSON
form and is `EncodeError.InvalidValue`. Text is escaped through `std.json.quote`; encoding a
`String` produces a new JSON token and does not expose mutable aliases.
`codec.optional_json(value)` encodes an `Option` (`null` for `None`) and
`codec.array_json(values)` an `Array` of any `Serialize` element type.
`EncodeError.UnsupportedType` is reserved for a later dynamic codec boundary;
unsupported derived fields will remain compile-time diagnostics.

`EncodeError.OutputLimit`, `DepthLimit`, and `InvalidValue` normalize bounded
JSON-object failures at the typed codec boundary. Derived serializers therefore
do not expose `std.json.JsonError` directly.

## Derived structs

Import `std.codec`, annotate a non-generic named struct, and call the generated
method normally. Fields are required, emitted in source order, and use their
source name unless `@json(name="...")` overrides the wire key.

```quazi
import std.codec;

@derive(Serialize)
struct Request {
    enabled: bool @json(name="active"),
    offset: i64,
    label: String,
}

const request: Request = Request {
    enabled: true,
    offset: -42,
    label: String.from("Ada"),
};
const encoded: String = request.to_json().unwrap();
// Exactly: {"active":true,"offset":-42,"label":"Ada"}
```

Unsupported fields (borrowed `str`, enums, generic structs, nested `Option` or
`Array` such as `Array[Array[i64]]`) are rejected with a field-level compiler
diagnostic. The generated method emits a fresh quoted token for every `String`
field and moves every other field out of the consumed receiver. A type
must choose either `@derive(Serialize)` or its own `impl Serialize`: combining
them is rejected as a duplicate `Type.to_json` declaration, diagnosed at the
derive attribute.

## Decoding

`decode_bool(source)` accepts exactly JSON `true` or `false` after JSON syntax
validation (including legal leading/trailing JSON whitespace). It returns
`DecodeError.InvalidJson` for malformed input and `DecodeError.TypeMismatch`
for a valid non-boolean JSON value. `decode_bool_with_limits(source, max_input,
max_depth)` preserves resource failures as `InputLimit` and `DepthLimit`:

```quazi
const enabled: bool = codec.decode_bool("false").unwrap();
```

`decode_string(source)` and `decode_string_with_limits` expose the same typed
boundary for one JSON string token. They preserve the JSON module's Unicode,
surrogate-pair, and embedded-NUL guarantees while normalizing failures to
`DecodeError`.

`decode_i64(source)` and `decode_i64_with_limits(source, max_input, max_depth)`
decode one JSON integer into a signed 64-bit Quazi value. The input must first
be syntactically valid JSON and then fit the `i64` decimal range; legal JSON
fractions and exponent forms such as `1.0` and `1e0` are numbers but are not
integer values, so they return `DecodeError.InvalidValue`. A valid non-number
token returns `TypeMismatch`, malformed JSON returns `InvalidJson`, and the
limit-aware form preserves `InputLimit` and `DepthLimit`.

```quazi
const offset: i64 = codec.decode_i64("-42").unwrap();
```

`decode_i8`, `decode_i16`, `decode_i32`, `decode_u8`, `decode_u16`,
`decode_u32`, `decode_u64`, `decode_isize`, and `decode_usize` decode a JSON
integer of that width; an integer outside the width's range is
`InvalidValue`. `decode_f64` and `decode_f32` accept any JSON number within
their finite range; a larger one such as `1e999` is `InvalidValue`.
`array_items(source)` returns the raw JSON text of each element of an array
(`TypeMismatch` for another JSON value).

## Derived decoding

`@derive(Deserialize)` on a non-generic struct generates an associated
function `Type.from_json(source: str) Result[Type, DecodeError]`. Fields follow
the [field matrix](#derived-field-matrix); `@json(name="...")` renames the key as
for `Serialize`. The object policy (maintainer answer
В19В) is strict rather than silent:

- a key the struct does not declare is `UnknownField`;
- a repeated key is `DuplicateField`;
- a missing required field is `MissingField`;
- an `Option` field is `None` when its key is missing or its value is
  `null`;
- a value of the wrong JSON type is `TypeMismatch`.

### Derived field matrix

| Field type | JSON | Serialize | Deserialize |
| --- | --- | --- | --- |
| `bool`, integers, `f32`, `f64` | number or boolean | `to_json` | `decode_<type>` |
| `String` | string | through its `str` view | `decode_string` |
| struct `S` | object | `S` must implement `Serialize` | `S.from_json` must exist |
| `Option[T]` | `T` or `null` | `null` for `None` | `None` when missing or `null` |
| `Array[T]` | array of `T` | `codec.array_json` | each element decoded as `T` |

`T` above is any non-container row (a scalar, `String`, or a struct).

```quazi
import std.codec;

@derive(Deserialize)
struct Profile {
    name: String,
    age: Option[i64],
}

fn main() i32 {
    const profile = Profile.from_json("{{\"name\": \"Bo\"}}").unwrap();
    if profile.age.is_some() { ret 1; }
    ret 0;
}
```

For hand-written decoders, `field_bool`, `field_i64` and `field_string`
decode a required field from `object_fields` spans (`MissingField` when
absent), and `optional_bool`, `optional_i64` and `optional_string` return
`None` for a missing or `null` field.

The generated function calls ordinary `std.codec` helpers:
`object_fields(source, keys)` finds each expected key in one pass,
`field_raw`/`optional_raw` return a field's raw JSON value, and the element
decoders above convert it. Each field is decoded by its own generated private
method, so the size of every generated function is independent of the number
of fields. Individual keys can also be read
with `json.object_field(source, key)`, which returns `Option` for a missing
key. Since a trait method needs a receiver, `from_json` is an associated
function rather than a `Deserialize` trait method. `EncodeError.message()` and
`DecodeError.message()` provide display text; callers should make policy from
the structured variants instead of matching the message.
