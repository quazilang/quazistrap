# `std.json`

`std.json` is the low-level JSON foundation for Quazi serialization. It is safe
to use as a bounded syntax gate for untrusted input and to encode individual
string/boolean/null tokens. It does not yet decode a JSON value or serialize a
struct; those APIs arrive with the compiler-backed `Serialize` and
`Deserialize` derives described in [D-012](../decisions/serialization.md).

## Validation

`validate(source)` checks that `source` contains exactly one JSON value with a
1 MiB input limit and 64 levels of array/object nesting. Use
`validate_with_limits(source, max_input, max_depth)` where the caller owns the
resource policy:

```quazi
import std.json;

fn accepts_payload(payload: str) bool {
    ret json.validate_with_limits(payload, 65536, 16).is_ok();
}
```

Validation accepts JSON strings, numbers, objects, arrays, `true`, `false`,
and `null`. It rejects malformed escape sequences and numbers, unclosed input,
trailing data, input over the byte limit, and nesting over the configured
container-depth limit. It validates syntax only: it does not allocate a JSON
DOM or establish application-level field policy.

## Encoding tokens

`quote(value)` returns one complete JSON string token. It preserves valid UTF-8
text and escapes quotes, backslashes, and C0 control characters. The return is
an owned `String` because escaping can grow the output.

```quazi
const encoded: String = json.quote("Ada\\nLovelace");
// encoded is `"Ada\\nLovelace"` as JSON source text.
```

`boolean(true)` and `boolean(false)` return the JSON literals `true` and
`false`; `null()` returns `null`. Numeric writing and complete object/array
construction are intentionally not yet public contracts.

## Error handling

Validation returns `Result[bool, JsonError]`. `JsonError` distinguishes input
and nesting limits from malformed string, escape, number, token, unexpected-end
and trailing-data failures. Applications should map these errors to their own
transport policy without assuming that a syntactically valid payload is trusted
or schema-valid.
