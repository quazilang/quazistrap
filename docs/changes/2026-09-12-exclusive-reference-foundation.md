# Exclusive-reference foundation

Quazi now parses and type-checks `&T!` and `&value!` as an exclusive
safe-reference form. An exclusive address-of requires a mutable local variable
or parameter, and `*reference = value` is permitted. The local borrow checker
rejects owner reads, moves, mutation, and overlapping shared or exclusive
loans after that exclusive loan begins.

This is deliberately a conservative source-level foundation, not completion of
D-014. Loans currently last through their enclosing lexical scope; there are
no inferred use-based control-flow regions, reborrows, receiver
capabilities, interprocedural effects, structural destruction, or QZI ownership
summaries. Existing reference escape restrictions remain in force.

The exclusive marker is part of the reference notation; `mut` remains an
ordinary identifier. `!` is not otherwise a postfix expression operator, so
`&value!` is unambiguous.

Verification includes semantic regressions for valid mutable dereference,
immutable roots, mutation through an outstanding exclusive loan, and conflicts
between shared and exclusive loans, followed by the compiler test suite.
