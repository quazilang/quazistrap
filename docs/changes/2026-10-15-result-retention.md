# Retained borrowed aggregate roots

Source helpers returning aggregates now compose conservative parameter
provenance before lexical borrow checking. Previously, wrapping an owner's
string view in a returned struct could erase that root, allowing the owner to
be released before the field was observed. Structs, enum payloads, nested and
generic wrappers, forwarding helpers, and outward assignments now retain their
roots and produce S10 when the owner would become unavailable.

The source evaluator distinguishes actual returned dependencies from unused
arguments and ignored calls, and models assignments, branch/loop unions,
receiver slots, named arguments, and conservative writes. Callee capability
queries use callee binders. Intrinsic/foreign bodies cannot prove result
independence when lowering overrides those bodies. The compiler-known fresh
allocation behavior of `quazi.str_concat` preserves valid owned copies.

Synchronous aggregate observation, static-only helper results, lexical loan
release, and the existing exact structural receiver-field transfer remain
supported. Unavailable source summaries do not prove aggregate result
lifetimes. These facts do not certify general escape/effect behavior or permit
QZI-only borrowed reference calls.

QZI stays v13 with Unverified ownership envelopes. QZC v14 invalidates older
exact hits and reusable units so accepted source is checked again. See the
[implementation contract](../internals/source-result-retention.md).

Verification includes dependency and borrow unit regressions, full compiler
checks, and `scripts/check_result_retention.py`: rejected source fails checking,
native building, and QZI emission; valid source executes natively and from QZI,
and QZI emits Windows COFF. No Windows execution is claimed. The wider D-014,
D-015, standard-library migration, and workspace readiness goals remain open.
