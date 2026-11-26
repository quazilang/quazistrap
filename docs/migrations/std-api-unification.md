# Unified std API conventions (В35А)

Date: 2026-11-26

Audience: Quazi users of `std.io`, `std.fs`, `std.os`, `std.stream`,
`std.tls`, `std.dylib`, `std.codec` and `std.collections`.

The maintainer chose to make the std API follow one convention per concern
(answer В35А to the std API audit). Most changes break existing code; the
compiler reports each affected call as a type error.

| Before | Now | Change your code |
|---|---|---|
| `io.readkey()` at end of input: `Ok("")` | `Err(ReadError.EndOfInput)`, like `read`/`readln` | match `Err(ReadError.EndOfInput)` instead of testing for an empty string |
| `stream.IoError.message()` → `String` (with the code or `Other` text) | `message(self) str` (static text, like every other error); `detail(self) String` keeps the old text | use `detail()` where the native code or `Other` text matters |
| `fs.current_dir()`, `os.current_dir()` → `Result[String, FsError]` | `Result[Path, FsError]`, like `canonicalize` and `DirEntry.path()` | add `.as_str()` (or `.to_string()`) |
| `fs.walk(root)` → `Result[Array[String], FsError]` | `Result[Array[Path], FsError]` | `entry.as_str()` for the relative path text |
| `DynamicLibraryError.OpenFailed` (Linux, NetBSD) / `Native(code)` (Windows) for a load failure | `OpenFailed(code)` on every target: the Windows `GetLastError` code, 0 where the loader gives none | match `OpenFailed(_)`; `Native(code)` now only reports unload failures |
| `TlsConfig.with_ca_file(path) TlsConfig`; `""` silently meant the system store | `Result[TlsConfig, TlsError]`; `""` is `Err(TlsError.InvalidConfig)` | add `?`/`.unwrap()`; use `TlsConfig.system()` for the system store |

Additions (not breaking):

- `HashMap.get(&key) Option[V]` copies a `Copy` value like `Map.get`; for
  values that own resources keep `find` + `value_at` (a by-value read of an
  owned value is rejected, S10).
- Every scalar decoder in `std.codec` has a `_with_limits(source, max_input,
  max_depth)` form (`decode_u64`, `decode_f64`, `decode_i8` ... `decode_u32`,
  `decode_isize`, `decode_usize`, `decode_f32`, `decode_system_time`), not
  only `decode_bool`/`decode_string`/`decode_i64`.

`stream.BufWriter` flushes when dropped (2026-11-26, D-028), like
`fs.BufferedWriter`: its `free` hook passes the buffered bytes on and flushes
the inner writer, ignoring errors. Call `flush()` to see errors;
`into_inner()` moves the inner writer out without the drop-time flush.
