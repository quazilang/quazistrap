# Exact JSON container-depth limits

`std.json.validate_with_limits` now counts every array or object toward its
`max_depth` limit. A scalar consumes no container depth, a single object or
array requires a limit of at least one, and each nested array or object
requires one additional level.

## Why

The prior check admitted one more container level than the configured limit.
`object_with_limits` also checked each field value in isolation, so it could
miss the enclosing object when enforcing a nesting policy. These were contrary
to the bounded-validation contract used for untrusted JSON and generated
serialization output.

## Compatibility and migration

Programs that intentionally set a limit one lower than their JSON container
depth now receive `JsonError.DepthLimit`, as documented. Raise `max_depth` to
the number of array/object layers the input or composed output actually needs.
No valid output representation or error variant changed.

## Verification

Source-level standard-library regressions cover scalar depth zero, a single
container at depth one, nested containers at depth one, and composed objects
that must account for their wrapper. `qz test` passes all 17 standard-library
tests.
