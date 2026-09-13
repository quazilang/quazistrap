# Typed JSON codec foundation

Audience: language users and compiler maintainers.

## Change

`std.codec` now exposes a `Serialize` trait plus `EncodeError` and `DecodeError`
vocabularies. `bool`, `str`, and `String` implement `Serialize`; `decode_bool`
and resource-bounded `decode_bool_with_limits` are the first typed JSON decode
functions. All decode input uses the bounded `std.json` implementation.

The codec now also exposes `decode_string` and `decode_string_with_limits`.
Unicode string decoding, object field extraction, and their raw JSON errors are
implemented in `std.json`; codec normalizes string failures to `DecodeError`.

## Compatibility

This checkpoint was additive: it initially did not make either derive generate
an implementation. The later `Serialize` checkpoint now generates serializers
for its documented bounded field matrix. `Deserialize` remains unavailable and
is rejected rather than accepted as a no-op: Quazi traits currently require a
receiver and cannot represent a static constructor safely. Adding receiverless
trait methods and bounded object-decoding policy are language-design
prerequisites, recorded here rather than shipping a non-callable trait surface.

## Verification

The codec smoke program checks boolean/text serialization and valid, malformed,
and mismatched boolean decode cases against the canonical standard library.
