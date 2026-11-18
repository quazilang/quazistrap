# `std.stream`

Audience: language users.

Status: initial (D-025, maintainer answer В26А). Generated signatures are in
the std repository's `docs/reference/stream.md`.

`std.stream` defines byte streams shared by files, TCP streams and the
standard streams, so generic code can move bytes without knowing where they
come from or go to.

## `IoError`

```quazi
pub enum IoError { Closed, WouldBlock, TimedOut, Interrupted, Native(i32), Other(String) }
```

Why a streaming operation failed. `message()` gives a short English
description. Other modules build values through the helpers `closed_error()`,
`would_block_error()`, `timed_out_error()`, `native_error(code)` and
`other_error(text)`.

## `Reader` and `Writer`

```quazi
pub trait Reader {
    fn read_bytes(self: &Self!, max: usize) Result[Array[u8], IoError];
}
pub trait Writer {
    fn write_bytes(self: &Self!, data: &Array[u8]) Result[usize, IoError];
    fn flush(self: &Self!) Result[bool, IoError];
}
```

- `read_bytes` returns up to `max` bytes; an **empty array means the end of
  the input**.
- `write_bytes` writes all of `data`, repeating partial native writes, or
  fails; it returns `data.len()`.
- `flush` pushes buffered bytes to their destination.

The methods are not named `read`/`write` because `File`, `TcpStream` and
`Stdout` already have methods of those names with other signatures.

| Type | Reader | Writer | Notes |
| --- | --- | --- | --- |
| `fs.File` | yes | yes | `flush` syncs the file to storage. |
| `net.TcpStream` | yes | yes | Receive failures are `Native(code)`; send failures map `NetError` to `TimedOut`, `WouldBlock`, `Closed` (reset or not connected) or `Other(message)`. |
| `io.Stdin` (`io.stdin()`) | yes | — | Raw bytes of file descriptor 0. |
| `io.Stdout`, `io.Stderr` | — | yes | Unbuffered; `flush` does nothing. |
| `stream.ByteSource` | yes | — | Reads an in-memory `Array[u8]`. |
| `stream.ByteSink` | — | yes | Collects bytes; `contents()` borrows them. |

## `copy`

```quazi
pub fn copy[R: Reader, W: Writer](reader: &R!, writer: &W!) Result[u64, IoError]
```

Moves every byte from `reader` to `writer` in 64 KiB chunks, flushes the
writer and returns the number of bytes copied. `io.copy` is the same
function.

```quazi
import std.stream;
import std.fs.File;

fn backup(from: str, to: str) Result[u64, stream.IoError] {
    var input = File.open(from).unwrap();
    var output = File.create(to).unwrap();
    ret stream.copy(&input!, &output!);
}
```

## Buffered adapters

`BufReader[R: Reader]` reads from its inner reader in large chunks (8 KiB by
default, `with_capacity` to choose) and serves small reads from memory. It is
itself a `Reader`, and `read_until(delimiter)` returns the bytes up to and
including the next delimiter (or the rest of the input; empty at the end).

`BufWriter[W: Writer]` collects writes and passes them on whenever the buffer
reaches its capacity. **Buffered bytes reach the inner writer only on
`flush` or when the buffer fills**: there is no flush when the writer is
dropped, because a failure could not be reported. `buffered()` is the number
of waiting bytes; `into_inner()` passes them on and returns the wrapped
writer.

`into_inner()` on `BufReader` returns its reader and drops bytes still
buffered.

## Limits

- `Array[u8]` stores one byte per 8-byte element slot today, so the stream
  implementations pack data into a contiguous buffer (`packed_bytes`) for
  every native write. Large transfers therefore allocate per chunk.
- Readers and writers are used through generic bounds; `dyn Reader` is not
  provided yet.
- `TcpStream.connect` needs the system linker (it calls `getaddrinfo`).
