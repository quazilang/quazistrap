# `std.codec`

`std.codec` defines the typed boundary above `std.json`. Its current scope is
deliberately small: `Serialize` is usable for booleans and text, while decoding
is exposed through explicit scalar functions. Struct derives and general typed
decoding are not implemented yet.

## Encoding

`Serialize.to_json()` produces one owned JSON value token:

```quazi
import std.codec;

fn main() i32 {
    const enabled: String = true.to_json().unwrap();
    const label: String = "ready".to_json().unwrap();
    // `enabled` is `true`; `label` is `"ready"` as JSON source text.
    ret 0;
}
```

The current implementations cover `bool`, `str`, and owned `String`. Text is
escaped through `std.json.quote`; encoding a `String` produces a new JSON token
and does not expose mutable aliases. Current scalar encoders do not fail.
`EncodeError.UnsupportedType` is reserved for a later dynamic codec boundary;
unsupported derived fields will remain compile-time diagnostics.

## Decoding

`decode_bool(source)` accepts exactly JSON `true` or `false` after JSON syntax
validation (including legal leading/trailing JSON whitespace). It returns
`DecodeError.InvalidJson` for malformed input and `DecodeError.TypeMismatch`
for a valid non-boolean JSON value. `decode_bool_with_limits(source, max_input,
max_depth)` preserves resource failures as `InputLimit` and `DepthLimit`:

```quazi
const enabled: bool = codec.decode_bool("false").unwrap();
```

The language currently requires every trait method to have a receiver, so it
cannot express a sound static `Deserialize.from_json(source)` trait method.
Decode entry points remain explicit functions until receiverless trait methods
are added to the language. This is a language limitation, not a promise that
generic `Deserialize` already works.

`DecodeError` also reserves limit, missing/duplicate/unknown field, and
invalid-value variants for bounded struct decoding. Those cases are not yet
produced by the current scalar decoder.
