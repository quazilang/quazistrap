# Migrating owned formatting results

Audience: Quazi users and library maintainers.

The D-013 contract changes `Display.to_string()` from `str` to `String`.
Integer, floating-point, boolean, and text implementations now construct real
owners. `String` also provides inherent borrowed `to_str()` and owned
`to_string()` methods. Update custom trait implementations to return `String`.

```quazi
const number: String = (42 as i32).to_string();
const message: String = fmt.format("value = {}", number);
const view: str = number.as_str();
```

Returning or passing an owner to an owned parameter transfers it. Formatting
borrows owners and copies their text; the owner remains usable afterwards.
Discarded and nested temporary owners are destroyed at the end of their
expression, after borrowed uses. Keep a named owner when retaining a view:
`const view = (42 as i32).to_string().as_str()` is rejected because its owner
would already be destroyed. Returning a view of a local owned string is also
rejected. Borrowed numeric `to_str()` still uses static scratch storage; it is
appropriate for immediate copies, not independent retained conversion results.

`std.core.str_concat`, `int_to_str`, `float_to_str`, and `str_from_byte` also
return `String`. The last accepts ASCII only, maps zero to empty text, and
panics for non-ASCII bytes. These APIs terminate on allocation failure; they
do not return a fallible allocation result.

Consuming source `free(self: T)` hooks for indirect source structs must return
`void`. The compiler releases the receiver's object storage after the hook
releases its resources, including early returns. Structural cleanup releases
the supported parent storage after its fields. A fallible or owner-returning
operation should use another method name. Borrowed `free(&T!)` methods and raw
allocator `free(ptr)` remain separate contracts.

`Index.index` now declares a shared receiver, matching the existing borrowed
`[]` operator behavior. Custom implementations use `self: &Self` (substituted
with their concrete implementation target). The prelude Array and String
implementations retain their owner after indexing; their old by-value wrapper
could destroy it during a read once receiver storage cleanup became effective.

`std.fs.File.close` uses `&File!` to invalidate its handle and preserve the
owner for later cleanup. Call it on a mutable binding. This also prevents
recursive `File.free` calls during by-value cleanup. Argument-taking native
entry points now transfer a heap-owned Array header to main.

## Compiled dependencies

QZI v11 records the original ownership ABI generation in content-bound
metadata. Namespace rewriting and reserialization preserve that origin.
Older artifacts using owned values, destructor/conversion hooks, allocation
instructions, or ambiguous public ownership contracts require a source rebuild.
Compatible primitive/raw-pointer artifacts remain readable. Merely
reserializing an old file into v11 does not make it compatible. QZC v9
invalidates older exact hits and partially reusable chunks automatically.

This compatibility boundary is not a D-014 ownership certificate. Current QZI
ownership envelopes remain unverified. Whole-program retention effects,
flow-sensitive borrowing, recursive generic/enum destruction, and unknown
returned aggregate/function capture provenance remain incomplete.
