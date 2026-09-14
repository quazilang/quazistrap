# PanicInfo receiver capabilities

`PanicInfo.message()`, `PanicInfo.file()`, and `PanicInfo.line()` now take a
shared `&PanicInfo` receiver.

## Why

Each accessor only reads a scalar or string view stored in the panic payload.
The shared receiver makes that non-consuming contract explicit and lets panic
handlers inspect the same `PanicInfo` through a borrowed view repeatedly.

## Compatibility and migration

Existing calls remain source-compatible. Custom panic handlers still receive a
`PanicInfo` value through the established terminal `fn(PanicInfo) !` ABI; this
does not add panic recovery, unwinding, or cleanup guarantees.

## Verification

A loader-backed compiler regression loads the actual prelude and checks all
three accessors through `&PanicInfo` in a valid terminal panic handler. The
compiler suite covers the receiver dispatch and panic-handler ABI checks.
