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

This is additive. It does not make `@derive(Serialize, Deserialize)` generate
an implementation, and it does not provide generic struct or text decoding.
`Deserialize` is intentionally absent: Quazi traits currently require a
receiver and cannot represent a static constructor safely. Adding receiverless
trait methods is a language-design prerequisite, recorded here rather than
shipping a non-callable trait surface.

## Verification

The codec smoke program checks boolean/text serialization and valid, malformed,
and mismatched boolean decode cases against the canonical standard library.
