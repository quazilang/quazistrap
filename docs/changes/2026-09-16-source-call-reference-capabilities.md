# Source-call reference capabilities

Audience: Quazi users and compiler maintainers.

Resolved, non-variadic safe unqualified source free-function calls now derive each argument
capability from the parameter declaration. An argument passed to `&T` or
`&T!` is borrowed or reborrowed for the call; it is not consumed merely because
the reference capability itself is affine. Owned parameters remain consuming.

Named arguments now undergo the same parameter-directed type validation as
positional arguments. Exact `&T!` arguments are accepted for exact `&T!`
parameters; shared references never gain exclusive capability.

## Boundary

This is a source-visible direct-call checkpoint, not D-014 completion.
Function-value, module-qualified and method, unsafe/foreign, variadic, and
QZI-only calls remain opaque and
do not receive a safe reference-bearing effect contract. QZI ownership
summaries, interprocedural fixed-point solving, escapes, and flow-sensitive
loan regions remain future work.
