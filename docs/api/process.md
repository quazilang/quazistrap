# `std.process`

Audience: Quazi application developers.

Status: Linux implementation available. `Command` adds PATH lookup, working
directories and environments at the maintainers' request
([D-021](../decisions/maintainer-answers-2026-11-10.md)); Windows process
creation and asynchronous extensions remain pending.

## `Command` (scripting)

`Command` builds and runs a program like a shell would, without a shell
(D-021). Builders take and return the command, so calls chain:

```quazi
import std.process;

fn main() i32 {
    const status = process.Command.new("sh")
        .arg("-c")
        .arg("exit 3")
        .current_dir("/tmp")
        .env("MODE", "test")
        .status();
    if (status.is_err()) { ret 1; }
    ret 0;
}
```

| API | Semantics |
| --- | --- |
| `Command.new(program)` | A program name without `/` is looked up in the `PATH` the child will see (an `env("PATH", ...)` override first); a path is used as is. |
| `arg(value)` / `args(values)` | Append arguments; `program` is `argv[0]`. |
| `current_dir(dir)` | Working directory of the child; a failing `chdir` is reported as `SpawnFailed`. |
| `env(name, value)` / `env_remove(name)` / `env_clear()` | Adjust the inherited environment; later calls win. |
| `spawn()` | Starts the child and returns a `Child`; a failed `execve` (missing program, no permission) is reported before returning. |
| `status()` | Spawns and waits, returning the `ExitStatus`. |

`Command` is Linux-only for now (fork/execve in the standard library; other
targets return `SpawnFailed(38)`); standard streams are inherited.

## Launching

`spawn(program: str, args: Array[str]) -> Result[Child, ProcessError]` starts
an exact executable path. It does not use a shell and does not search `PATH`.
`args` excludes `argv[0]`; the runtime supplies `program` as the child’s first
argument. Standard input, output, and error are inherited.

`str` values cross the current runtime boundary as NUL-terminated text, so a
NUL terminates the value observed by the operating system. Do not use text with
embedded NUL to construct a process path or argument; preserving such data
requires a future length-carrying process-input API. `SpawnFailed(code)` carries
the native error code. On Linux, the parent observes an `execve` failure before
`spawn` returns, so a successful `Child` always represents a process that
crossed the exec boundary.

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
| `try_wait()` | Borrows the child exclusively. It returns `Ok(None)` while live; once exited, it returns the status and clears the handle. The local remains valid and later `wait()`/`close()` calls return their documented no-op result. |
| `terminate()` | Forcefully requests termination through an exclusive receiver without consuming the child. Returns `Ok(false)` for a consumed child. |
| `close()` | Consumes the child. On Linux, a live child is killed and reaped; it can block. |
| `handle()` | Exposes the raw platform handle through a shared receiver for narrowly scoped target-specific interoperation. Do not close or wait on it outside `Child`. |

`ExitStatus.Code(i32)` is a normal exit code. Linux also reports
`ExitStatus.Signal(i32)` for signal termination. Windows will report only
`Code` when it is implemented.

## Errors and limits

`ProcessError` has `SpawnFailed(i32)`, `WaitFailed(i32)`,
`TerminateFailed(i32)`, and `CloseFailed(i32)`. `message()` produces a
display-oriented summary; `native_code()` exposes the native number.

Working-directory selection, custom environments, redirected pipes, captured
output, timeouts, and cancellation are not implemented. Native-library and
object-only Linux embeddings do not have Quazi’s captured startup environment;
their spawn attempt returns `SpawnFailed(ENOSYS)` rather than launching with an
empty environment. The complete design and portability requirements are in
[D-011](../decisions/process-runtime.md).
