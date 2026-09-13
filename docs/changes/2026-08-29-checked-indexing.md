# Checked indexing and contiguous fixed arrays

Audience: Quazi users and compiler/tooling developers.

Safe indexing now checks bounds for fixed arrays, slices, and `bytes` before
performing a load or store. The compiler uses one unsigned `index < length`
comparison, so negative signed indices are rejected as well as indices at or
above the length. Failure reports `index out of bounds` through the normal panic
runtime. Packages built with `std = false` use a deterministic machine trap.

Constant invalid fixed-array and byte-string indices are rejected during
semantic analysis. Built-in indexable values require exactly one index instead
of silently ignoring extra expressions. C flexible-array members remain an
explicitly unsafe, unchecked operation because their length is not represented.

Fixed arrays live in contiguous QZI register blocks. `Lea` now carries an
opcode-scoped block length, allowing validation and both register-allocation
passes to preserve every element. This replaces the old instruction-neighbor
heuristic that could mistake the element-size constant `8` for the array length
and corrupt arrays longer than eight elements.

This is a safety-compatible source change for valid programs. Programs that
relied on out-of-bounds access now fail deterministically, and programs using
multiple indices on a built-in value must select one index explicitly. The
additional comparison adds a small branch cost to dynamic safe indexing;
constant valid fixed-array indices continue to lower directly.

Fixed arrays cannot currently cross a function boundary by value. The old
single-register call ABI copied only their first element, so parameters and
returns of `[T; N]` are now rejected with a clear diagnostic instead of silently
corrupting data. Use `Array[T]` for a function-owned collection until a borrowed
fixed-array ABI is defined.
