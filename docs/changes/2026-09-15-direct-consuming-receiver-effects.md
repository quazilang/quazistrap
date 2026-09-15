# Direct consuming receiver effects

Bare inherent-method receivers (`self: T`) now consume a move-only owner at a
resolved direct call. Explicit `self: &T` and `self: &T!` receivers retain
their existing shared and exclusive call-local loan behavior.

## Scope

The compiler derives this effect from the method declaration, not from names
such as `free` or `close`. A consuming call moves a whole local owner, blocks
when a shared or exclusive loan is live, and transfers cleanup responsibility
to the callee. Scalars and other plain-copy receivers remain usable as before.

Place-level moves are not implemented yet. A consuming receiver cannot be a
field, indexed element, or dereference of a move-only aggregate; it may own a
whole local or a temporary value. Dynamic dispatch, indirect
calls, cross-call effect solving, structural destruction, and QZI/QZC ownership
summaries remain outside this checkpoint.

## Compatibility

Methods declared with bare move-only `self: T` now invalidate their local
receiver after a successful direct call. APIs which only observe or mutate an
owner should declare `self: &T` or `self: &T!` instead. This checkpoint does
not migrate all legacy standard-library receiver declarations.

## Verification

Semantic regressions cover use-after-consume, loan conflicts, and the
place-move boundary. Bytecode coverage verifies cleanup is transferred for an
arbitrarily named consuming method rather than a special `free` path. The full
offline compiler suite passes.
