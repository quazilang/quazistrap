# Dynamic-library receiver capabilities

`std.dylib.DynamicLibrary.symbol` and `raw_handle` now take shared receivers.
They inspect an already-open native library without changing its ownership or
the platform handle.

## Why

Symbol lookup and raw-handle interoperation must not race a safe close through
the same owner. The explicit shared receiver makes that temporary loan visible
to the current borrow checker, as part of the staged migration preceding
D-014's consuming bare `self: T` semantics.

## Compatibility

Existing calls on a local library owner remain valid; a `const` library binding
can now perform lookup as well. `close` and `free` retain their legacy
by-value spelling until consuming receiver effects are implemented.

## Verification

The contained compiler's standard-library source tests compile the dynamic
library module, and the canonical documentation suite validates the API page.
