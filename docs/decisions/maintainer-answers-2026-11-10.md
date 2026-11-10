# D-021: maintainer answers of 2026-11-10

Audience: compiler and standard-library developers.

Status: decided by the maintainers on 2026-11-10 (asks.txt rounds 4-5).

- **В9А.** Numeric `to_str()` returns an owned `String`, like `to_string`;
  the static per-site digit buffer goes away and `Display.to_str` returns
  `String`.
- **В10А.** A by-value `match` consumes its scrutinee: bindings own the
  payload, the rest is destroyed, and the scrutinee cannot be used after it
  (use-after-move). `match &value` borrows instead.
- **В11В.** Variadics must behave as ordinary typed variadics; formatting
  must not depend on a compiler "format function" crutch (`@format`, a
  `str` parameter before `...: str`). Format-capable APIs format through
  ordinary language means (D-020 interpolation).
- **В12Б.** Zed runtime validation is still required (maintainers check it).
- **В13Б.** Wine does not count as Windows; a second developer runs the
  matrix on real Windows.
- **В14А.** A closure passed directly to `thread.spawn(...)` is a one-shot
  closure like `qo`'s and takes ownership of what it captures.
- **В15Г.** Numeric `as` casts between integers and floats are to be
  implemented fully.
- **Requests.** `std.fs` gains a complete file-system surface (`Path`/
  `PathBuf`, `read_dir` with `ReadDir`/`DirEntry`, `FileType`, `metadata`,
  `is_dir`/`is_file`, `canonicalize`, `create_dir_all`), and `std.process`
  gains `Command` (`arg`, `args`, `current_dir`, `env`, `spawn`, `status`,
  `PATH` lookup) so Quazi can replace shell scripts. This request supersedes
  D-011's deferral of working directories, environments and `PATH` lookup.
