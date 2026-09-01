# Unicode-safe JSON string decoding

Audience: language users and compiler maintainers.

## Change

`std.json.decode_string` and `decode_string_with_limits` now decode one exact
JSON string token into owned Quazi text. They reuse bounded syntax validation,
decode all JSON short escapes and `\uXXXX`, combine valid UTF-16 surrogate
pairs, preserve embedded NUL, and reject lone or malformed surrogate halves.

## Compatibility and verification

This is additive. It does not decode JSON objects or arrays yet. Focused native
smoke coverage verifies escapes, Unicode, surrogate errors, embedded NUL,
non-string rejection, and input limits; an independent review checked buffer
capacity, ownership transfer, bounds, and UTF-8 validity.
