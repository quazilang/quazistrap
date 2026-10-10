# Native ownership fixtures

Audience: compiler and prelude contributors.

Run from the compiler root with the matching canonical standard-library
checkout. Each standalone program exits zero on success. These supplement
inline Rust unit tests with native execution; they are not automatically
discovered by `cargo test`.

```sh
target/debug/qz build prelude/tests/owned-display.qz --no-incremental -o /tmp/qz-owned-display -r
target/debug/qz build prelude/tests/owned-format.qz --no-incremental -o /tmp/qz-owned-format -r
target/debug/qz build prelude/tests/owned-output.qz --no-incremental -o /tmp/qz-owned-output -r
target/debug/qz build prelude/tests/owned-args.qz --no-incremental -o /tmp/qz-owned-args -r
target/debug/qz build prelude/tests/owned-file.qz --no-incremental -o /tmp/qz-owned-file -r
```

`owned-output` prints exactly `literal=42`, `dynamic=43`, and `writer=44`,
one per line. `owned-file` reads this checkout's fixture and explicitly closes
the same File twice before automatic cleanup. `owned-args` transfers entry
arguments to another function and indexes them without destroying their owner.

Use `-i` to emit portable QZI, then compile/run that artifact for a round trip.
Use `-c --target x86_64-windows` for COFF emission on Linux; this verifies
emission, not Windows execution. For allocator accounting, Linux `strace -e
trace=mmap,munmap` on `owned-format` should show equal mapping/unmapping counts.
Enum shell and general scalar-only aggregate destruction remain incomplete;
do not infer their leak freedom from the format fixture.
