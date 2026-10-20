# Migrating typed `?` propagation

Audience: Quazi users upgrading source code or rebuilding dependencies.

`?` now enforces the [wrapper and error-type contract](../language/expressions.md#error-propagation).
If analysis reports `S14`, check the return type of the nearest callable:

- An `Option[T]` operand requires an `Option[U]` return type.
- A `Result[T, E]` operand requires `Result[U, E]`, with the same error type
  after aliases resolve. Use `match` and an explicit supported conversion or
  error mapping when the target error type differs.
- A scalar or void-returning function must handle the failure locally with
  `match`, or change its signature and callers to return the appropriate wrapper.
- A closure cannot return from its enclosing function. Handle failures inside
  scalar closures. Wrapper-returning closures remain unavailable until closure
  result ownership is implemented.
- For a qualified constructor with no resolved result type, create a typed
  local using the bare contextual constructor, then apply `?` to that local.

Success types may differ, for example `Result[i32, Error]` can propagate into
`Result[bool, Error]`. Transparent wrapper aliases are accepted by `?` and by
subsequent unqualified enum matches.

Source caches older than QZC19 are invalidated automatically. Rebuild older QZI
dependencies from source to obtain the new checks; emitting a native binary
from an old QZI body does not re-typecheck its source or repair its behavior.
QZI13 remains Unverified. See the
[change record and verification](../changes/2026-10-20-typed-try-propagation.md).
