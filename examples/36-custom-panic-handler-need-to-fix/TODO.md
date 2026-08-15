# TODO

- The compiler reports duplicate definitions of `PanicInfo` and `__quazi_panic_handler` because they exist both in `prelude/src/panic.qz` and `~/.quazi/std/src/panic.qz`. The std/prelude architecture needs to be cleaned up so `panic` is only defined in one place.
- `eprintln` is missing from `io`, causing compilation errors in the panic handler.
