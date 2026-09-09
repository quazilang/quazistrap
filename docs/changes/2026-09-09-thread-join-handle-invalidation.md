# Thread join handle invalidation

Date: 2026-09-09

Audience: users of the experimental `std.thread` API and tooling maintainers.

`Thread.join()` now clears its wrapper's handle before calling the native join
primitive. The compiler currently passes method receivers as aliases, so the
previous wrapper could retain a native handle after Linux freed its pthread
storage or Windows closed it. A second wrapper call could therefore reuse an
invalid native resource despite the API documenting `join` as consuming.

Compatibility: `Thread.join()` remains unsafe and returns `void`. Repeating it
through the same wrapper now reaches the low-level zero-handle no-op. Extracted
raw handles remain an unsafe interoperability escape hatch and must not be
joined independently of the wrapper.

Verification:

```bash
cd ../std
qz test thread --no-color
```

This only verifies the package-level compiler path. Native thread execution
still requires an explicit external linker and pthread library on Linux, and
the higher-level concurrency model remains experimental.
