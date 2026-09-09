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

## Contract details still requiring maintainer approval

The runtime boundary is accepted, but its public `std.process` contract is not
yet implementable without the following choices. They must be resolved together
because they determine resource ownership, failure reporting, and QZI runtime
compatibility.

1. **Argument convention and executable lookup.** Decide whether the argument
   list excludes `argv[0]` (the runtime supplies the program as element zero)
   or includes it, and whether `program` is an exact executable path or follows
   a documented `PATH` lookup policy. The Windows command-line marshaller must
   use the same logical argument sequence as Linux `execve`.
2. **Exit status.** Define a portable structured result that distinguishes a
   normal numeric exit from termination by a POSIX signal. A raw packed integer
   would lose information and make Windows and Linux behavior ambiguous.
3. **Termination.** Define the first portable termination request: graceful
   request, forced termination, or both. Linux signal choice and Windows
   `TerminateProcess` behavior have materially different cleanup guarantees.
4. **Close and destruction.** Decide what `Child.close()` means while a Linux
   child is still running. Linux has a PID rather than a closeable process
   handle; dropping it without a later wait can leave a zombie, while an
   implicit wait can unexpectedly block. The compiler's automatic `free(self)`
   convention cannot be assigned a behavior until this policy is explicit.
5. **Spawn result ABI.** Define a POD out-record or equivalent separate output
   fields for the opaque child handle and native error. A one-slot return value
   cannot safely encode both a Windows `HANDLE` and an error code. The raw
   runtime intrinsic must receive borrowed program/argument data only for the
   call duration; it must not depend on the implementation-defined `Array`
   object layout.

After approval, the implementation needs a UTF-16 `CreateProcessW` marshaller
with explicit Windows quoting and an inherited-standard-handle list, plus a
Linux `fork`/`execve` path that reports child `execve` failures to the parent
before reporting spawn success. New intrinsic IDs also need the matching QZI
compatibility and validation update.

## Verification requirements

The runtime implementation must test executable paths and arguments containing
spaces, quotes, and non-ASCII text; non-zero exits; repeated `wait` and
`close`; spawn failures; and compile/run coverage for every supported target.
No standard-library-only fork/exec or `CreateProcess` wrapper may be presented
as cross-platform support before those runtime primitives exist.
