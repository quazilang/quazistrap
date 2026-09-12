# `std.process`

Audience: Quazi application developers.

Status: Linux implementation available. Windows process creation is not yet
implemented, so applications requiring Windows support must not depend on this
module.

## Launching

`spawn(program: str, args: Array[str]) -> Result[Child, ProcessError]` starts
an exact executable path. It does not use a shell and does not search `PATH`.
`args` excludes `argv[0]`; the runtime supplies `program` as the child’s first
argument. Standard input, output, and error are inherited.

Embedded NUL bytes in `program` or any argument return `InteriorNul` before an
operating-system call. `SpawnFailed(code)` carries the native error code. On
Linux, the parent observes an `execve` failure before `spawn` returns, so a
successful `Child` always represents a process that crossed the exec boundary.

```quazi
import std.process;

fn run_tool() void {
    var args: Array[str] = Array.new();
    args.push("-i");
    const result = process.spawn("/usr/bin/env", args);
    if (result.is_err()) { panic(result.unwrap_err().message()); }
    var child = result.unwrap();
    const status = child.wait();
    if (status.is_err()) { panic(status.unwrap_err().message()); }
}
```

The executable pathname in the example is platform-specific. It is not a
recommendation to construct a command string or rely on a shell.

## `Child` ownership

`Child` owns one native child handle. Its methods have these effects:

| Method | Behavior |
| --- | --- |
| `wait()` | Blocks for completion, returns `Ok(Some(status))`, and consumes the child. A consumed child returns `Ok(None)`. |
| `try_wait()` | Returns `Ok(None)` while live. Once exited, returns its status and consumes the child. |
| `terminate()` | Forcefully requests termination without consuming the child. Returns `Ok(false)` for a consumed child. |
| `close()` | Consumes the child. On Linux, a live child is killed and reaped; it can block. |
| `handle()` | Exposes the raw platform handle for narrowly scoped target-specific interoperation. Do not close or wait on it outside `Child`. |

`ExitStatus.Code(i32)` is a normal exit code. Linux also reports
`ExitStatus.Signal(i32)` for signal termination. Windows will report only
`Code` when it is implemented.

## Errors and limits

`ProcessError` has `InteriorNul`, `SpawnFailed(i32)`, `WaitFailed(i32)`,
`TerminateFailed(i32)`, and `CloseFailed(i32)`. `message()` produces a
display-oriented summary; `native_code()` exposes the native number, or zero
for `InteriorNul`.

Working-directory selection, custom environments, redirected pipes, captured
output, timeouts, and cancellation are not implemented. Native-library and
object-only Linux embeddings do not have Quazi’s captured startup environment;
their spawn attempt returns `SpawnFailed(ENOSYS)` rather than launching with an
empty environment. The complete design and portability requirements are in
[D-011](../decisions/process-runtime.md).
