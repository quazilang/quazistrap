# D-028: a `free` hook, then the remaining fields

Decided 2026-11-26 by the maintainer (asks В36А).

A struct's consuming `free(self: T)` hook no longer has to release every
owned field itself. Destruction of a value of type `T` is:

1. the hook runs (if `T` declares one);
2. the compiler destroys, in reverse declaration order, every owned field
   the hook did not move out (`self.text.free()`, `const f = self.file;`
   and passing a field by value all move it);
3. the aggregate's storage is released.

Before, step 2 did not exist: fields the hook left behind leaked (for
example a `String` field of a struct whose hook only closed a handle).

A by-value receiver of the type's own method (`fn into_inner(self: T) W`)
may move fields out even when `T` has a hook, including on only some paths.
When a field has moved out, the hook does not run for that receiver (the
value is no longer whole); the remaining fields are destroyed one by one. A
runtime mask in the receiver's drop guard makes the choice exact per path.
Every other owner of a hooked type (locals, parameters other than the
method's own `self`, fields) is still destroyed whole and cannot have fields
moved out of it (S10).

Consequences:

- `std.stream.BufWriter` flushes on drop (its hook calls `flush`), and
  `into_inner` moves the inner writer out, which skips the hook.
- Existing hooks that release their fields explicitly keep working: each
  explicit release is a move, so nothing is destroyed twice.
- Field destructors behind a hook (generic specializations such as
  `Box[T].free<Res>` included) are destructor roots of the semantic plan.

Scope limits (unchanged): `repr(C)` structs, recursive layouts and types the
structural plan cannot describe keep the old behavior (hook, then storage
only). Rust-style "no moves out of a type with a destructor" was the
alternative and was not chosen.

Checked by `scripts/check_drop_hooks.py` (native, QZI, `--valgrind`).
