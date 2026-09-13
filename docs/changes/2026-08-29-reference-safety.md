# Lexical shared-reference safety

Quazi previously allowed values to become references, widened pointee types,
took the address of temporary result registers, returned stack addresses, and
allowed register allocation to reuse an address-taken slot.

The compiler now uses a conservative lexical reference contract. `&` accepts a
local or parameter place, reference compatibility is directional and invariant,
and non-string references cannot escape through returns, owned aggregates, or
closures. Reference bindings cannot be rebound. The ownership pass keeps an
address-taken owner borrowed for the rest of the function and rejects mutation,
method use, or moves that could invalidate the reference. Stores through nested
shared-reference lvalues are rejected as well.
Aggregate dereference/materialization is also rejected: current aggregate
values retain address semantics, so a shallow load would create a mutable alias
despite the shared reference. Scalar/value-like dereference remains available.

Scalar address-of emits `Lea.flags = 1`; larger register blocks and variadic
packs emit their exact lengths. Register compaction and linear scan therefore
reserve every addressed slot. QZC v3 invalidates stale exact-hit caches, and QZI
artifacts with implicit zero-length `Lea` metadata require a source rebuild.

Regression coverage includes value-to-reference rejection, invariant pointees,
invalid temporary/field/index address-of forms, reference escape and rebinding,
closure capture, shared-owner mutation, scalar slot pinning, QZI rejection, and
a native register-pressure execution test. The full offline suite passes 410
tests.
