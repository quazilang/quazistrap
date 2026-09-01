# D-011: child-process creation belongs to the runtime

Status: accepted 2026-09-01.

## Context

Quazi needs a shellless, cross-platform way to start a program and observe its
exit status.  Linux can express this with `fork`, `execve`, `wait4`, and file
descriptor syscalls.  Windows instead requires `CreateProcessW` and a set of
structured, mutable UTF-16 buffers.  Implementing both directly in the
standard library would duplicate platform ABI details and make correct argument
quoting and lifetime handling impossible to validate behind one language-level
interface.

The current `Array[String]` ownership model also does not yet provide the
stable builder semantics needed for an owned command configuration.  The old
Windows declarations are therefore not a safe foundation for a public process
API.

## Decision

The compiler runtime will provide the platform-specific creation, argument
marshalling, waiting, termination, and handle-closing primitives.  The standard
library will expose the public `Process`, `Child`, `ExitStatus`, and
`ProcessError` types over those primitives.

The first public surface will be deliberately small:

- it starts an executable without invoking a shell;
- it accepts a program path and argument list for the duration of the call;
- children inherit the parent's standard streams;
- `wait`, `try_wait`, `terminate`, and `close` have explicit ownership and
  repeated-call behavior.

Working-directory selection, custom environments, redirected pipes, captured
output, timeouts, and cancellation are deferred until the basic handle and
ownership contract is implemented and tested on each supported target.

## Verification requirements

The runtime implementation must test executable paths and arguments containing
spaces, quotes, and non-ASCII text; non-zero exits; repeated `wait` and
`close`; spawn failures; and compile/run coverage for every supported target.
No standard-library-only fork/exec or `CreateProcess` wrapper may be presented
as cross-platform support before those runtime primitives exist.
