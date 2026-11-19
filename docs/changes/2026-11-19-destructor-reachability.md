# Destructors follow live types; methods on unit variants

Date: 2026-11-19

Audience: Quazi users and compiler developers.

## Destructor reachability

Generated cleanup calls a type's consuming `free(self)` hook without a call
edge, so every `free` method used to be compiled into every program. A
library type whose destructor calls a foreign function (for example
`std.tls.TlsStream`, which calls OpenSSL) then made every program that loaded
the module reference those foreign symbols, even without using the type.

Executables and test binaries now compile `T.free` only when reachable code
can hold a `T`: some expression inside a reachable callable (or a global
initializer) has a type that mentions `T`, a reachable callable's parameter or
result type mentions it, or it is a field, payload or alias of such a type.
Newly rooted destructors can make further types live; the compiler repeats
until nothing changes. Names are compared by their last path segment, which
can only keep more destructors. Library and QZI builds still root every
destructor. If code generation ever needs a hook that was not compiled, the
build fails with an internal error instead of skipping the cleanup.

## Methods on unit enum variants

`Color.Red.name()` panicked in code generation: the receiver chain was taken
for the module `Color` and the call lost its receiver. A module-call fallback
now applies only to a bare module name.

Both have regressions in `bytecode::codegen::tests`.
