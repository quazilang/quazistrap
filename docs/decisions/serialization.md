# D-012: serialization uses static typed derives, not runtime reflection

Status: accepted 2026-09-01.

## Context

Quazi has no public serialization library or generated serializer today.
Unknown field attributes are retained by the parser and QZI metadata, but they
are not values that ordinary Quazi code can inspect. `@derive(...)` is recorded
by semantic analysis but does not synthesize a trait implementation.

The proposed path of general compile-time reflection, an arbitrary comptime
interpreter, and comptime-generated declarations would be a new language
metaprogramming system. It is not a prerequisite for useful, type-safe
serialization and should not be introduced merely to avoid writing a focused
derive implementation.

## Decision

The first supported serialization surface is a compiler-backed, static derive:

```quazi
@derive(Serialize, Deserialize)
struct AddArgs {
    force: bool @json(name="force"),
    output: String @json(name="output"),
}
```

It generates ordinary typed implementations of standard-library traits for
supported aggregate fields. The initial wire format is JSON. The first release
is intentionally limited to non-generic named structs with `bool`, `i64`, and
owned `String` fields. `Option` and nested derived structs are not yet part of
the field matrix. Field order follows source declaration order; `@json(name="...")`
changes only the JSON key. Unknown input keys, missing required fields,
duplicate keys, defaults, enum tagging, numeric ranges, recursion and
allocation limits are explicit codec-contract questions that must be specified
before implementation.

Generated implementations are compiler-owned metadata/codegen, not runtime
reflection. They have no `Type` value, no ability to enumerate arbitrary types
at runtime, and no ability to generate unrestricted Quazi declarations. This
keeps binary size, ownership, error reporting, and compatibility auditable.

## Delivery sequence

1. Define `std.codec` traits, JSON value grammar, `EncodeError` and
   `DecodeError`, limits, duplicate/missing-field policy, and the supported
   primitive/collection field matrix.
2. Carry ordered aggregate-field names, resolved types, and opaque attributes
   into stable compiler derive metadata; reject misplaced or invalid `@json`
   arguments with source spans. Generate a synthetic semantic/codegen IR rather
   than mutating parsed source declarations.
3. Implement `@derive(Serialize)` for non-recursive structs and primitive
   fields, with deterministic JSON output and regression fixtures.
4. Implement bounded JSON decoding and `@derive(Deserialize)`, then enums
   (whose variant attributes are not retained today), generic/container types,
   and a dynamic JSON DOM only after ownership and destruction support is
   verified.

General compile-time reflection and comptime code execution remain separate
future language proposals. If introduced, they must be designed for more than
serialization and must define sandboxing, termination/resource limits,
determinism, QZI compatibility, diagnostics, and declaration hygiene.

## Compatibility and verification

The derive names and JSON field naming become public API only when steps 1–3
ship together. Each step requires source/QZI round trips, exact JSON fixtures,
malformed and adversarial decode cases, nested values, Unicode keys/strings,
numeric boundaries, cross-target tests, and a no-generated-code fallback
diagnostic for unsupported fields.

## Implementation checkpoint

Steps 2 and 3 are implemented for the initial `Serialize` matrix. The compiler
records ordered, type-alias-resolved metadata for `Serialize`/`Deserialize`
requests, validates the reserved `@json(name="...")` field attribute, and
lowers `@derive(Serialize)` into a compiler-owned ordinary `Serialize` impl
before semantic analysis. The parsed source AST remains immutable; generated
methods then use the normal type-checking, dependency, and bytecode paths.

The JSON composition prerequisite for step 3 is now available as
`std.json.object(keys, values)`: it quotes keys, validates each raw value token,
preserves source-provided ordering, rejects duplicate keys, and applies a
bounded aggregate-output policy. The first generated-serialization matrix is
now fixed to `bool`, `i64`, and owned `String` fields only. Each field is
required, encoded under its source or `@json(name=...)` key, and emitted in
source declaration order. Borrowed `str`, other integer widths, floats,
`Option`, collections, and nested structs remain unsupported until their
ownership and error contracts are added deliberately. Decode policy, including
unknown and missing fields, remains a step-4 prerequisite.

An explicit `impl Serialize for Type` conflicts with the generated method and
is rejected as a duplicate `Type.to_json` declaration. The diagnostic points to
the `@derive(Serialize)` attribute, since it is the source request that caused
the compiler-owned implementation.
