# Deterministic JSON object composition

Audience: Quazi users and serialization implementers.

`std.json` now provides `object(keys, values)` for composing one JSON object
from ordered key strings and pre-encoded JSON value tokens. It quotes every key
with the canonical JSON string encoder, validates each value before appending
it, and keeps the caller's order. This provides a shared composition boundary
for the planned compiler-generated `Serialize` implementations, so the compiler
does not duplicate JSON escaping or grammar logic.

The function has a 1 MiB aggregate-output limit and depth limit of 64;
`object_with_limits` accepts explicit limits. It returns
`JsonError.ObjectLengthMismatch` for unequal arrays, `DuplicateKey` for
repeated keys, `OutputLimit` when composition would exceed the output cap, and
the underlying validation error for an invalid value token. It is additive and
does not yet generate a struct serializer or define decode policies for unknown
or missing fields.

The same release adds the documented `json.null()` helper, which returns the
borrowed `null` token without allocation.

`std.codec.Serialize` now also supports `i64`, using its exact base-10 JSON
number spelling. This completes the standard-library side of the first derive
field matrix: `bool`, `i64`, and owned `String`.

The compiler now lowers `@derive(Serialize)` for non-generic structs in that
matrix into ordinary compiler-owned methods. Output is source-ordered, honors
`@json(name="...")`, and returns typed `EncodeError` values when bounded object
composition fails. Unsupported field types are rejected at their declaration.
An explicit `impl Serialize` for the same type is rejected as a duplicate
method, with the diagnostic located at `@derive(Serialize)` rather than a
synthetic compiler span.

Verification covers exact ordered output, escaping, empty objects, malformed
raw values, length mismatches, duplicate keys, and output-limit failures using
a native standard-library smoke program on Linux and Windows compile-only.
