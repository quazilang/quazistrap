# File receiver capabilities

`std.fs.File` now distinguishes operations that inspect its native handle from
operations that can advance or mutate the underlying file state. `fd` and
`raw_handle` use a shared receiver. Raw and text reads/writes, seeking,
synchronization, and truncation use an exclusive receiver.

## Why

File position and storage state must not be mutated while safe code holds a
shared loan of the owning handle. The explicit receiver declarations let the
current compiler enforce that call-local boundary and are part of the staged
standard-library migration required before bare `self: T` can acquire its
planned consuming D-014 meaning.

## Compatibility

Calls on mutable local `File` values remain source-compatible. Code that keeps
a shared `&File` loan may continue to inspect `fd` or `raw_handle`, but must end
that loan before calling a read, write, seek, sync, or truncate operation.
`close` and `free` retain their existing legacy receiver spelling during the
broader consuming-receiver migration; this record does not claim their D-014
consumption effect is implemented yet.

## Verification

The standard-library project passes semantic checking with the contained
compiler. The runnable file-I/O tutorial fixture is compiler-checked and
lowered to bytecode by the canonical documentation test suite.
