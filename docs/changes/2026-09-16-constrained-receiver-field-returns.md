# Constrained receiver-field returns

Audience: Quazi compiler and standard-library maintainers.

A by-value inherent method may now return an owned direct field with exactly
`ret self.field` when its receiver is an eligible source struct and the field
already has compiler-generated structural cleanup. The return copies the field
to the result before cleanup; cleanup then destroys only the receiver's other
eligible fields.

The exclusion is local to that `return`, rather than changing the receiver's
stored drop state. This keeps later returns on another control-flow branch
correct. The bridge rejects aliases, nested projections, wrappers, generic and
`repr(C)` parents, scalar fields, and parents with a manual `free(self)` hook.
All other partial moves remain S10 errors.
