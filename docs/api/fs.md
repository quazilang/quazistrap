# `std.fs`

Audience: Quazi application developers.

`std.fs` provides the currently supported cross-platform file and directory
operations for Linux and Windows. Paths are UTF-8 `str` values passed to the
native platform APIs; there is no separate `Path` type, buffering layer,
metadata object, recursive traversal, or atomic-write helper yet.

## Errors

Every fallible public operation returns `FsError`. `message()` is suitable for
display, not for branching; match error variants when application behavior must
differ.

| Variant | Meaning |
| --- | --- |
| `NotFound`, `PermissionDenied`, `AlreadyExists`, `InvalidPath` | Normalized native failures. |
| `IsDirectory`, `NotDirectory`, `DirectoryNotEmpty`, `ReadOnly`, `NoSpace`, `TooManyOpenFiles` | Normalized file-system conditions where the platform reports them. |
| `AllocationFailed` | A standard-library staging allocation failed. |
| `InvalidData` | Text input was not valid UTF-8, or a native directory record was malformed. |
| `Unsupported` | The operation has no implementation for the selected target. |
| `Native(i32)` | An unclassified OS error code. Its numeric meaning is platform-specific. |

Normalization is necessarily incomplete: applications that require a portable
policy must handle `Native` conservatively.

## `File`

`File` owns one native file handle. Its destructor calls `free`, which closes
the handle on lexical exits when the compiler can discover the destructor.
Use explicit `close()` only when the resource must be released before the end
of scope. A successful close invalidates the value; a later automatic cleanup
is a no-op. `close()` returns `Ok(false)` for an already-invalid handle.

```quazi
import std.fs;

fn write_note(path: str) Result[bool, fs.FsError] {
    const opened = fs.File.create(path);
    if (opened.is_err()) { ret Err(opened.unwrap_err()); }
    const file = opened.unwrap();
    const written = file.write_str("hello\n");
    if (written.is_err()) { ret Err(written.unwrap_err()); }
    ret file.sync();
}
```

| API | Semantics |
| --- | --- |
| `File.open(path)` | Opens an existing file for reading. |
| `File.create(path)` | Creates or truncates a file for writing. On Linux the creation mode is `0o644` before process `umask`; Windows ignores that mode. |
| `File.open_rw(path)` | Opens read/write and creates the file when absent. It does not truncate an existing file. |
| `File.open_append(path)` | Opens for append and creates when absent. Linux uses append mode; Windows seeks to the current end at open time, so concurrent writers have platform-dependent append behavior. |
| `file.read(buf, n)` | Unsafe raw read into a caller-owned writable buffer. It returns a byte count, zero at EOF, or a negative failure sentinel. The caller supplies valid storage and handles partial reads. |
| `file.write(bytes)` | Performs one binary write and returns the number of bytes written. A successful short write is possible; callers needing complete output must retry the remainder. |
| `file.write_raw(buf, n)` | Unsafe counterpart for an arbitrary readable byte buffer. It can perform a partial write. |
| `file.write_str(text)` | Performs one UTF-8 text write and returns bytes written. It does not append a terminator or retry a partial write. |
| `file.seek(offset, whence)` | Changes the current position and returns the resulting byte offset. Use `seek_set()`, `seek_cur()`, or `seek_end()` for `whence`; invalid offsets and non-seekable files return `FsError`. |
| `file.sync()` | Requests that file contents reach the platform’s persistent-storage boundary. It does not promise hardware durability beyond the operating system’s API. |
| `file.truncate(len)` | Changes a file length on Linux. It currently returns `Unsupported` on Windows. |
| `file.fd()` / `file.raw_handle()` | Expose native handles for tightly scoped interoperation. `fd()` truncates Windows handles and is therefore not a portable handle API; prefer `raw_handle()` only with target-specific FFI. |
| `file.close()` / `file.free()` | Releases the native handle as described above. |

`File` is an owning resource, not a thread-safe shared handle abstraction. Do
not duplicate or retain raw handles beyond the file’s lifetime.

## Whole-file text

`read_to_string(path)` reads to EOF into an owned `String`. It grows an internal
buffer, closes the file on success and failure, and validates the complete byte
sequence before constructing `String`. Invalid UTF-8 yields `FsError.InvalidData`;
use raw `File.read` for binary input. There is currently no caller-controlled
maximum size, so do not use it for untrusted or arbitrarily large files.

```quazi
const config: Result[String, fs.FsError] = fs.read_to_string("settings.qz");
```

## Paths and directories

| API | Semantics and platform notes |
| --- | --- |
| `exists(path)` | Returns whether a native existence query succeeds. It conflates absence, permission failures, and other errors. |
| `readable(path)` / `writable(path)` | Best-effort access probes, not a guarantee that a later open or write will succeed. Windows `writable` currently reports existence rather than testing write access. |
| `count_entries(path)` | Counts immediate entries, excluding `.` and `..`; it does not recurse. Native enumeration errors are returned. |
| `remove(path)` | Removes a file, not a directory. |
| `mkdir(path)` | Creates one directory. On Linux it requests `0o755` before `umask`; Windows ignores POSIX mode bits. Parent directories are not created. |
| `mkdir_mode(path, mode)` | Linux-specific requested mode; Windows creates a directory but does not apply the mode. |
| `rmdir(path)` | Removes an empty directory. |
| `rename(from, to)` | Renames a path using platform native semantics. Replacement and cross-volume behavior are platform-specific. |
| `link(existing, new_path)` / `symlink(target, link_path)` | Create a hard link or symbolic link on Linux. They currently return `Unsupported` on Windows. |
| `chmod(path, mode)` | Changes POSIX mode bits on Linux and returns `Unsupported` on Windows. |

These APIs are subject to time-of-check/time-of-use races. Do not authorize a
security-sensitive operation based on `exists`, `readable`, or `writable` and
then assume a subsequent path operation affects the same object.

## Limits and future work

The module has no path normalization policy, file metadata API, buffered
reader/writer interfaces, streaming trait integration, directory iterator,
large-file limit control, or cross-platform symbolic-link and permission model.
Those are not implied by the current functions and should be treated as future
design work rather than emulated through undocumented raw-handle behavior.
