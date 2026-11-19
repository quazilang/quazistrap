# Panic handling

Audience: Quazi users and runtime implementers.

Panics terminate the current process. Quazi currently has no unwinding,
recovery, or destructor guarantee after a panic; use `Result` for expected
failures.

## Custom handlers

The prelude supplies `PanicInfo`, containing the panic message, source file,
and source line. Its `message()`, `file()`, and `line()` accessors take a
shared `&PanicInfo` receiver, so a handler can inspect one borrowed view more
than once. A program may replace the default process-terminating formatter
with exactly one handler:

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

## Lifecycle

This is the fatal-panic contract of [D-006](../decisions/README.md#d-006-panic-model)
(maintainer answer В28):

- The handler runs at most once per process, for a fatal panic: a panic in
  `main` or in code it calls outside qoroutines. It receives the panic's
  `PanicInfo` and must end the process (its result type is `!`).
- The stack is not unwound and local destructors are not guaranteed to run.
- Without a custom handler the default prints the report and a backtrace and
  exits with status 101.
- A panic inside a qoroutine is not fatal and never reaches the handler: the
  default report (`qoroutine panicked at '...'`) is printed, only that
  qoroutine ends, and its `join()` returns `Err(QoroutineError.Panicked)`
  ([D-019](../decisions/qoroutines.md)), with or without a custom handler.

Handlers are intended for terminal diagnostics. A panic raised while a panic
is being handled (for example by the handler itself) does not call the handler
again: the program prints `panicked while processing a panic: '<message>'`
with its location and exits with status 101 (maintainer answer В16А). Inside a
qoroutine this applies per qoroutine. Allocation failure while reporting,
concurrent panics in several OS threads (D-007), and cleanup ordering do not
yet have a stronger public guarantee. A custom handler is not a recovery hook and
must not be used to establish application control flow.
