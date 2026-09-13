# Custom panic handlers now require the runtime ABI

Audience: Quazi users and runtime maintainers.

`@panic_handler` now requires one non-generic, non-variadic exact signature
`fn(PanicInfo) !`. Earlier validation also accepted multiple handlers, `str`,
arbitrary named types, references, variadic/generic forms, and `void` returns
even though the runtime constructs one `PanicInfo` value and never has a valid
continuation after invoking the handler. Those signatures could compile but
receive an ABI-incompatible value or return into a terminal panic path.

This is a source compatibility correction. Replace a loose handler parameter
with `PanicInfo` and make the handler terminate with `!`; handlers that were
already written to the documented prelude contract require no change. Quazi
still provides no panic recovery or unwinding contract.

Verification includes semantic regressions for rejected duplicate, `str`,
unrelated same-shaped aggregate, variadic, generic, and `void` handlers, while
retaining the exact `PanicInfo`/`!` form. The full compiler test suite also
covers the change.
