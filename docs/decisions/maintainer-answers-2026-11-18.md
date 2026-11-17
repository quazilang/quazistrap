# D-025: maintainer answers of 2026-11-18

Status: decided by the maintainers on 2026-11-18 (asks.txt В23-В26).

- **В23А.** Generic values move: inside generic code a parameter `T` is
  move-only unless it is bounded by `T: Copy`. `Copy` is a compiler-known
  marker trait: integers, floats, `bool`, `str`, byte strings, shared
  references, raw pointers and C function pointers are `Copy`. The
  maintainers also asked for a working `Copy` trait with `@derive(Copy)` for
  structs and for an `AsRef` trait with `.as_ref()`. Reading an owned `T`
  out of a shared reference (`ret self.value;` with `self: &S[T]`) is an
  error that suggests a `Copy` bound or returning `&self.value`; contiguous
  containers read elements by value only when the concrete element is
  `Copy`, and gain a borrowing accessor (`get_ref`) for the rest.
- **В24 → В27.** Enum serialization is to be configured through attributes
  (like `@json(name=...)` on fields); the concrete attribute set is asked in
  В27.
- **В25А.** Time zone data is discovered from the system IANA database
  (`$TZDIR`, `/usr/share/zoneinfo`, `/etc/zoneinfo`,
  `/usr/share/lib/zoneinfo`); updates come with the operating system and
  nothing is bundled. This resolves D-009; `std.timezone` implements it.
- **В26А.** Streaming I/O uses `Reader`/`Writer` traits with a common
  `IoError` (`Closed`, `WouldBlock`, `TimedOut`, `Interrupted`, `Native`,
  `Other`), a generic `io.copy`, and buffered adapters, implemented for files,
  TCP streams and the standard streams; an empty read means end of input.
