# Partial-move safety guard

The compiler now rejects a move-only value selected from a field, indexed
element, or safe-reference dereference whenever that expression is consumed.
This applies uniformly to function arguments, returns, assignment values,
aggregate construction, consuming method receivers, and other consuming
expression positions.

## Motivation

The current cleanup implementation records ownership for whole locals. It
cannot yet suppress cleanup for only a moved field, array element, or enum
payload. Accepting such a move would therefore allow the containing aggregate
to later destroy storage that had already been transferred.

## Compatibility

Programs that previously moved an owned projection now receive `S10`. Move the
whole owner, pass a reference where the callee permits one, or keep the value
in place until place-level moves and structural destruction are implemented.
Copyable scalar projections remain accepted.

## Scope and verification

This is a conservative D-014 safety boundary, not an implementation of
place-level move tracking or recursive destruction. Semantic regressions cover
field, fixed-array index, and safe-dereference moves, plus an allowed scalar
projection. The full compiler suite remains the release-level check.
