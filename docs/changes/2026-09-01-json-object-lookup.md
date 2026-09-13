# Bounded JSON object field lookup

Audience: language users and compiler maintainers.

## Change

`std.json.object_field` now extracts the raw validated JSON token for one key
from a top-level object. It decodes escaped Unicode keys before comparison,
enforces configured input/depth limits, and returns `None` for an absent key.

Duplicate occurrences of the requested decoded key are rejected with
`JsonError.DuplicateKey`; no implementation-defined last-key-wins behavior is
allowed. The validator now rejects unpaired UTF-16 surrogate escapes anywhere,
so validation and decoded key lookup share one Unicode invariant.

## Verification

Smoke tests cover escaped keys, matching decoded duplicates, nested raw values,
non-object input, depth limits, and malformed surrogates. Independent review
checked raw token boundaries, duplicate behavior, source ownership, and the
surrogate invariant.
