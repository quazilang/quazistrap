# `std.process`

Audience: Quazi application developers.

Status: implemented on Linux, NetBSD and Windows (Windows through
`CreateProcessW`, checked under Wine; NetBSD shares the Linux fork/execve
implementation through `std.posix` and libc and is build-checked only, by
`std/scripts/check_process_targets.py`). `Command` adds PATH lookup, working directories and
environments at the maintainers' request
([D-021](../decisions/maintainer-answers-2026-11-10.md)); asynchronous
extensions remain pending.

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
| `env(name, value)` / `env_remove(name)` / `env_clear()` | Adjust the inherited environment (the current one, including `os.set_env` changes); later calls win. On Windows names compare ASCII case-insensitively and the child gets an ANSI environment block. |
| `spawn()` | Starts the child and returns a `Child`; a failed `execve` (missing program, no permission) is reported before returning. |
| `status()` | Spawns and waits, returning the `ExitStatus`. |

`Command` uses fork/execve on Linux and `CreateProcessW` with a UTF-16
command line on Windows; standard streams are inherited.

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
| `free()` | Scope cleanup: a child that was neither waited for nor closed is closed when its owner leaves scope (a live child is killed and reaped). Close errors are discarded; call `close()` to observe them. |
| `handle()` | Exposes the raw platform handle through a shared receiver for narrowly scoped target-specific interoperation. Do not close or wait on it outside `Child`. |

`ExitStatus.Code(i32)` is a normal exit code. Linux also reports
`ExitStatus.Signal(i32)` for signal termination. Windows reports only
`Code`.

## Errors and limits

`ProcessError` has `SpawnFailed(i32)`, `WaitFailed(i32)`,
`TerminateFailed(i32)`, and `CloseFailed(i32)`. `message()` produces a
display-oriented summary; `native_code()` exposes the native number.

Working directories, custom environments, captured output (`output()`), and
timeouts (`wait_timeout`, `wait_or_terminate`) are implemented; arbitrary
standard-stream redirection beyond `output()` is not. Native-library and
object-only Linux embeddings do not have Quazi’s captured startup environment;
their spawn attempt returns `SpawnFailed(ENOSYS)` rather than launching with an
empty environment. The complete design and portability requirements are in
[D-011](../decisions/process-runtime.md).
