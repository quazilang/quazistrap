# Checked dense-element address foundation

Date: 2026-09-17

Audience: compiler maintainers.

The compiler now has a private `ContiguousElementAddr` bytecode operation for
the future D-015 borrowed dense-container element contract. It receives the
storage base, requested index, initialized length, and the resolved non-zero
element slot stride. Native lowering checks the index before calculating an
address and traps on an out-of-bounds request.

## Compatibility and migration

This is private, un-emitted compiler infrastructure: no source-level accessor
or public borrowed-element API emits the operation, so it does not alter the
new QZI v10/QZC v8 ownership-artifact contract beyond the mandatory unverified
envelope and users have no source migration work.
When D-015 begins to emit it, D-014 requires a new QZI/QZC ownership-summary
boundary and a source rebuild of artifacts that lack those summaries.

## Verification

Focused regressions verify that a zero stride is rejected during QZI
validation, all four register operands survive inlining and allocation,
constant propagation invalidates the pointer destination, and SysV and Win64
lowerings perform their bounds check before either scale path.

## Remaining work

The remaining D-015 work must add compiler-validated accessor metadata,
provenance-aware borrow checking, multi-slot dereference support, and the D-014
QZI/QZC ownership-summary compatibility boundary before source code can use
the operation.
