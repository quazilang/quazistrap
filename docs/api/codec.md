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

The current implementations cover `bool`, `i64`, `str`, and owned `String`.
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

The language currently requires every trait method to have a receiver, so it
cannot express a sound static `Deserialize.from_json(source)` trait method.
Decode entry points remain explicit functions until receiverless trait methods
are added to the language. This is a language limitation, not a promise that
generic `Deserialize` already works.

`DecodeError` also reserves limit, missing/duplicate/unknown field, and
invalid-value variants for bounded struct decoding. Those cases are not yet
produced by the current scalar decoder.
