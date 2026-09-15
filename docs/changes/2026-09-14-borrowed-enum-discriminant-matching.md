# Borrowed enum discriminant matching

`match` may now inspect an enum variant through `&Enum` or `&Enum!` when every
payload position is a wildcard or the variant has no payload. The prelude's
read-only `Option` and `Result` predicates now use this capability and accept
shared views.

## Why

Variant tags are read-only metadata. Inspecting them does not materialize an
aggregate or create a payload alias, so it is sound before the broader D-014
aggregate-projection model lands.

## Compatibility and migration

Existing enum matches keep their behavior. `Option.is_some`, `Option.is_none`,
`Option.ok`, `Result.is_ok`, `Result.is_err`, and `Result.ok` can now be called
through a shared view. Payload inspection through a borrowed enum remains a
compile-time error: `Some(value)`, `Some(1)`, and nested payload patterns are
all unavailable. Use a value match when ownership transfer is intended.

## Verification

Semantic regressions distinguish tag-only borrowed matches from rejected
payload bindings. A loader-backed regression checks the actual prelude's
generic `Option` and `Result` predicates through shared views, and bytecode
coverage lowers a borrowed enum discriminant match.
