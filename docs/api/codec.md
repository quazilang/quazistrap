# `std.codec`

`std.codec` defines the typed boundary above `std.json`. Its current scope is
deliberately small: `Serialize` is usable for booleans, signed 64-bit integers,
and text, while decoding is exposed through explicit scalar functions.
`@derive(Serialize)` supports non-generic structs whose fields are all `bool`,
`i64`, or owned `String`; general typed decoding is not implemented yet.

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

The current implementations cover `bool`, `i64`, `str`, owned `String`, and
`time.SystemTime`, which encodes as an RFC 3339 UTC JSON string such as
`"2000-02-29T12:34:56Z"`; `codec.decode_system_time(source)` decodes it
(numeric offsets are normalized to UTC, malformed text is `InvalidValue`).
`i64` uses its exact base-10 representation. Text is escaped through
`std.json.quote`; encoding a `String` produces a new JSON token and does not
expose mutable aliases. Current scalar encoders do not fail.
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

Borrowed `str`, other integer widths, floats, options, collections, nested
structs, and enums are rejected with a field-level compiler diagnostic. The
generated method emits a fresh quoted token for every `String` field. A type
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

## Derived decoding

`@derive(Deserialize)` on a non-generic struct generates an associated
function `Type.from_json(source: str) Result[Type, DecodeError]`. Fields may
be `bool`, `i64`, `String`, or `Option` of those; `@json(name="...")`
renames the key as for `Serialize`. The object policy (maintainer answer
В19В) is strict rather than silent:

- a key the struct does not declare is `UnknownField`;
- a repeated key is `DuplicateField`;
- a missing required field is `MissingField`;
- an `Option` field is `None` when its key is missing or its value is
  `null`;
- a value of the wrong JSON type is `TypeMismatch`.

`Serialize` does not yet encode `Option` fields.

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

The generated function calls ordinary `std.codec` helpers:
`object_fields(source, keys)` finds each expected key in one pass and
`field_*`/`optional_*` decode the values. Individual keys can also be read
with `json.object_field(source, key)`, which returns `Option` for a missing
key. Since a trait method needs a receiver, `from_json` is an associated
function rather than a `Deserialize` trait method. `EncodeError.message()` and
`DecodeError.message()` provide display text; callers should make policy from
the structured variants instead of matching the message.
