# Panic handling

Audience: Quazi users and runtime implementers.

Panics terminate the current process. Quazi currently has no unwinding,
recovery, or destructor guarantee after a panic; use `Result` for expected
failures.

## Custom handlers

The prelude supplies `PanicInfo`, containing the panic message, source file,
and source line. A program may replace the default process-terminating
formatter with exactly one handler:

```quazi
import std.core;

@panic_handler
fn handle_panic(info: PanicInfo) ! {
    // Report `info` using only operations safe in a terminal failure path.
    core.exit(101);
}
```

The program may contain at most one non-generic, non-variadic handler. Its
single parameter must be exactly `PanicInfo` (not `str`, a reference, a type
alias, or another same-shaped struct), and its result type must be `!`. The
compiler installs the handler under the runtime panic entry point and passes
one `PanicInfo` value using Quazi's ordinary value ABI. A handler that returns
could resume into a panic path with no valid continuation, so it is a
compile-time error.

Handlers are intended for terminal diagnostics. Recursive panics, allocation
failure while reporting, concurrent panics, and cleanup ordering do not yet
have a stronger public guarantee. A custom handler is not a recovery hook and
must not be used to establish application control flow.
