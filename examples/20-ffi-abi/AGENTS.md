# C ABI phase-two round trip

This fixture crosses the C boundary in both directions. It prints a short
self-test introduction, one PASS or FAIL line for each check, and a final
summary. A successful run exits with status zero; failures print the affected
ABI feature and use exit codes 1 through 9.

- `Point` exercises two-register SSE aggregates and a scalar `f32` argument.
- `Sample` exercises an eight-byte mixed float/integer aggregate plus `f32`
  field conversion.
- `Triple` exercises memory/hidden-sret aggregate passing.
- `c_sum8`/`quazi_sum8` exercise stack overflow arguments in both directions.
- `BinaryCallback` passes an exported Quazi function to C and calls a C-returned
  function pointer through the same portable source on SysV and Win64.
- `c_global_counter` and `c_global_ratio` exercise unsafe typed reads and writes
  of external C data symbols, including `f32` boundary conversion.
- C calls exported Quazi adapters before returning each value to Quazi, so the
  same source checks `@api` and `@export` symmetrically.

Run `qz run` from this directory. On Windows, use an LLVM `clang` as `CC` and
`lld-link` as the Quazi linker. On Linux, use Clang or GCC plus `ld.lld`.

Generate the declarations exported by this fixture without compiling or linking:

```text
qz header -o quazi-ffi.h --target x86_64-linux
qz header -o quazi-ffi.h --target x86_64-windows
```

The target controls `@cfg` selection, `c_long` widths, and aggregate spelling;
the generated header is guarded for both C and C++ consumers.
