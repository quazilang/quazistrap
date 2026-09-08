# Target-aware test discovery

`qz test` now loads a package entry through its normal target-aware import
graph before adding independent test roots. A source file beneath `src/` is an
additional root only when it declares an `@test` function. Files beneath
`tests/` remain roots, including malformed files, so test diagnostics are not
hidden.

Previously every source file was added as an independent root. That bypassed a
conditional import such as `@cfg(target_os="windows") pub import windows;` and
caused Linux standard-library test runs to analyze Windows-only FFI bindings.
No language semantics or standard-library API changed; the fix only restores
the import graph's configured target behavior during test discovery.

The runner now supplies its selected native target to the loader explicitly.
Regression coverage verifies that an unimported test remains discoverable while
a disabled Windows-only module is not loaded on Linux. The canonical command
`qz test ini --no-color --no-unicode` passes the five `std.ini` tests on Linux.

Compatibility: projects with test-bearing source files continue to have them
discovered. Unimported ordinary source files no longer affect `qz test`; they
must be reachable from the package entry to participate in a build, matching
normal project compilation.
