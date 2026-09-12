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

## Approved public contract

Approved 2026-09-10. The first surface is intentionally narrow:

- `program` is an exact executable path; there is no `PATH` lookup or shell.
- `args` excludes `argv[0]`; the runtime inserts `program` as element zero on
  Linux and applies the same logical sequence to Windows command-line quoting.
- Standard streams are inherited. Working directory, environments, pipes,
  captured output, timeouts, and cancellation remain deferred.
- `ExitStatus` distinguishes `Code(i32)` from `Signal(i32)`. Windows reports
  only `Code`; Linux reports a terminating signal without exposing packed
  `waitpid` bits.
- `terminate` is forced (`SIGKILL` / `TerminateProcess`) and does not consume
  the child.
- `wait` and an exited `try_wait` consume the child. `close` terminates a live
  child then waits/reaps it, so it may block but cannot leave a Linux zombie.
  Repeated operations and handle `0` are no-ops.
- Runtime intrinsics return only `0` or `-1` and write handle, status, and
  native error through separate scalar out-pointers. They receive a borrowed
  `(args_ptr, args_len)` slice, never an implementation-defined `Array` object.

The implementation needs a UTF-16 `CreateProcessW` marshaller
with explicit Windows quoting and an inherited-standard-handle list, plus a
Linux `fork`/`execve` path that reports child `execve` failures to the parent
before reporting spawn success. New intrinsic IDs also need the matching QZI
compatibility and validation update.

Linux compiler-generated executable startup retains the kernel `envp` vector,
which is the environment passed to `execve`. Native-library and object-only
embeddings do not run that startup path. Before `std.process` is shipped for
such an embedding, the runtime must either receive its environment through an
explicit initialization hook or reject process creation with a documented
error; it must not silently launch a child with an empty environment.

## Verification requirements

The runtime implementation must test executable paths and arguments containing
spaces, quotes, and non-ASCII text; non-zero exits; repeated `wait` and
`close`; spawn failures; and compile/run coverage for every supported target.
No standard-library-only fork/exec or `CreateProcess` wrapper may be presented
as cross-platform support before those runtime primitives exist.
