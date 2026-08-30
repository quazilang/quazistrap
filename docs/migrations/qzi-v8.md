# Migrating compiled libraries to QZI v8

Audience: maintainers distributing compiled Quazi libraries.

QZI v8 is the phase-2 layout-query boundary. It assigns stable intrinsic IDs for
`quazi.size_of[T]()` and `quazi.align_of[T]()` so prelude allocation code can
ask the compiler for each concrete element layout instead of hardcoding an
eight-byte element size.

Rebuild `.qzi` artifacts with the current compiler before distributing them to
v8 consumers. Older compilers must reject v8 artifacts because they do not know
the layout-query intrinsic IDs. Current compilers continue to read compatible
older QZI artifacts subject to the v7 ownership, runtime-`any`, and explicit
`Lea` metadata restrictions documented in [QZI v7](qzi-v7.md).

No Quazi source changes are expected for ordinary scalar or handle-based
container code. Multi-register generic values such as `Array[[i32; 3]]` are
still rejected at generic function boundaries until the remaining phase-2
register-block ABI work lands.
