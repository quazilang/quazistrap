# Linux runtime-backed process support

Linux builds now provide the first D-011 child-process implementation through
compiler runtime intrinsics and `std.process`.

## Behavior

`process.spawn(program, args)` executes the exact `program` path without a
shell or `PATH` lookup. The runtime supplies `argv[0]`, inherits standard
streams, and uses a close-on-exec error pipe: a child whose `execve` fails is
reported as `SpawnFailed(native_errno)`, never as a successfully spawned
`Child`.

`wait`, `try_wait`, `terminate`, and `close` implement the ownership rules in
[D-011](../decisions/process-runtime.md). Linux status values distinguish exit
codes from terminating signals; close kills and reaps a live child.

The private runtime ABI uses scalar out-pointers rather than an `Array`
representation. QZI v9 and QZC v7 are the resulting artifact boundaries.

## Compatibility and limits

This adds the new `std.process` module on Linux. Windows is not yet supported:
its required UTF-16 `CreateProcessW` marshaller, Windows quoting, and strict
inherited-handle list remain work in progress. Applications that need Windows
process launching must not depend on this module yet.

Object-only and native-library embeddings do not execute Quazi's Linux startup
stub and therefore lack a captured environment vector. Spawn reports `ENOSYS`
there instead of silently creating a child with an empty environment.

## Verification

- `cargo test --offline --quiet` in `quazistrap` — 582 tests passed.
- `qz test` in `std` — 19 tests passed, including exact-path launch and failed
  `execve` regression coverage.
