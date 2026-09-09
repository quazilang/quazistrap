# Idempotent network socket close

Date: 2026-09-09

Audience: users of `std.net` and runtime maintainers.

`TcpStream.close()`, `TcpListener.close()`, and `UdpSocket.close()` now check
whether their wrapper has already been invalidated. The first close clears the
stored socket before invoking the native close operation; later closes are
no-ops. This also makes automatic `free()` safe after an explicit `close()`.

Compatibility: this is a behavioral safety fix with no source migration.
Programs that called `close()` or `free()` repeatedly no longer forward an
invalid socket value to the operating system. Raw handles remain a
target-specific interoperability escape hatch and are not independently owned
by the wrapper once extracted.

Verification:

```bash
cd ../std
qz test net --no-color
```

The focused test binds a local UDP socket and checks the observable invalidation
state across explicit `close()`, a repeated close, and `free()`. It cannot
observe a second native close directly because the low-level close operation
returns `void` and discards operating-system errors. Native lifecycle tracing
still needs a linked Quazi executable on each supported target.
