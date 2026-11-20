# `std.core`

Audience: Quazi application and standard-library developers.

`std.core` is the low-level intrinsic boundary below the safer `std.fs`,
`std.io`, `std.net`, and `std.os` APIs. Most functions are `unsafe` because
they accept raw pointers or caller-supplied lengths. Prefer higher-level modules
unless implementing a carefully bounded platform abstraction.

## Raw I/O

`unsafe write(fd, text, count)`, `write_bytes(fd, ptr, count)`,
`read(fd, ptr, count)`, and `stderr_write(text, count)` return a native signed
byte count or negative failure value. The caller must keep storage readable or
writable for the exact count; text operations also require valid UTF-8. They
do not retry partial I/O or normalize OS errors.

## Process and host intrinsics

`exit(code)` terminates the process and does not practically return despite its
legacy `i32` declaration. `sleep_ms(ms)` blocks subject to scheduling.
`unsafe getenv(name)` returns a borrowed, null-terminated native pointer or
null. `unsafe hostname(buf, capacity)` and `unsafe cpu_name(buf, capacity)`
write byte data and return its length or `-1`; callers own validation and
buffer capacity. `memory_total()` and `memory_available()` return bytes or zero
when the host query fails. `windows_build()` and `windows_product()` return
zero outside Windows.

Runtime-private operations are declared in `std.core` so other std modules
can reach them, but they are not application APIs: `process_spawn`,
`process_wait`, `process_try_wait`, `process_terminate` and `process_close`
(the child-process runtime behind `std.process`), `atomic_add` and
`atomic_cas` (behind `std.thread` atomics and locks), `context_switch`
(behind qoroutines) and `runtime_state` (the process runtime state block; see
[runtime state](../internals/runtime-state.md)). Their contracts can change
with the compiler runtime.

## Raw allocation and memory

`unsafe malloc(size)` returns an allocated block or null. `unsafe realloc(ptr,
size)` returns a replacement block or null; on success the old pointer must no
longer be used, and on failure it remains owned by the caller. `unsafe free(ptr)`
requires a pointer returned by the matching allocator and must be called at
most once. `memcpy`, `memmove`, `memset`, and `memcmp` have their ordinary C
contracts: all ranges must be valid, and only `memmove` permits overlap.

## Text and UTF-8

`strlen(text)` returns the UTF-8 byte length of a null-terminated `str`.
`str_byte_at(text, index)` is an `unsafe fn` without a bounds check: `index <
text.bytes_len()` is required, and a larger index reads outside the string.
`unsafe str_as_ptr(text)` borrows immutable storage and must not be freed or
written through. `unsafe str_from_ptr(ptr)` creates a borrowed view only after
the caller proves valid, null-terminated UTF-8 and lifetime.

`unsafe is_valid_utf8(ptr, len)` validates a readable byte range, rejecting
overlong encodings, surrogates, and incomplete sequences. `unsafe
utf8_incomplete_tail(ptr, len)` reports continuation bytes needed for an
otherwise valid trailing prefix; run full validation after completing the tail.

## Owned text results

`str_concat(a, b)` and `str_from_byte(u8)` return owned `String` values
(numbers become text with `n.to_string()`). Return or move the result to
transfer ownership; lexical cleanup releases it automatically. Borrow
`.as_str()` only while its owner remains alive. Allocation failure terminates
rather than returning invalid text; these convenience APIs are infallible.

`str_from_byte` accepts ASCII only. Zero produces an empty string, preserving
the former NUL-terminated behavior; bytes at or above 128 panic because a
single such byte is not valid UTF-8. Use `bytes` for arbitrary byte data.
See the [owned-formatting migration](../migrations/owned-formatting.md).
