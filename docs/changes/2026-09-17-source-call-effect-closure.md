# Source-call ownership-effect closure

The compiler now derives a conservative source-only fixed point before it
allows the existing temporary `&T` / `&T!` call loan to end when an
unqualified call returns. A callable is eligible only when it has a source
body, is safe, fixed-arity, and non-intrinsic, and every call in its body is
another eligible unqualified direct source call. Recursive groups are accepted
when that closed graph proves the same condition.

Calls through function values, closures, C ABI values, module-qualified or
method dispatch, unsafe or foreign declarations, and QZI declarations are
explicitly opaque for this checkpoint. Generic source calls retain their
existing template-level capability behavior, but do not establish a verified
QZI specialization boundary. Any opaque call removes the caller from the
eligible set. This deliberately uses a dedicated semantic call-effect record rather than the reachability graph,
which also contains function-value and compiler-generated dependencies.

This is a tightening of the existing lexical source-call rule, not D-014's
whole-program ownership solver. It neither permits reference escape nor
validates QZI ownership summaries; `transitive_effects_verified` remains
false for every callable.

Regression coverage verifies direct chains and recursive groups, while an
opaque declaration and a function-value callback both disqualify their
callers.
