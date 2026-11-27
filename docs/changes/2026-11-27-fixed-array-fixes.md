# Fixed arrays: nested indexing, owned elements, destruction

Date: 2026-11-27

Audience: Quazi users of fixed arrays (`[T; N]`).

Found by a crash-hunting review; all three produced wrong results in safe
code.

- Nested fixed arrays (`[[i64; 2]; 2]`) are laid out flat, but literals,
  indexing and element writes assumed one register per element:
  `m[0][1]` read `m[1][0]`, and `m[i][j] = v` with a dynamic `i` wrote into
  a temporary copy. Literals, reads, writes and compound assignment now use
  the element's slot count, with a bounds check at every level. Assigning a
  whole nested row (`m[i] = [..]`) is a compile error for now.
- An owned call result in an array literal (`[make_string()]`) stayed an
  expression temporary and was destroyed after the literal: the array held
  freed memory. It now moves into the array.
- Fixed arrays of owned elements were never destroyed (their elements
  leaked). The array is now destroyed with its owner, each element once;
  moving the array transfers that responsibility.
- `for value : array` over a fixed array of owned elements copied each
  element into the loop variable while the array still owned it, which
  read freed memory. It is rejected (S10); iterate by index. `Array[T]`
  keeps its consuming by-value loop.

Regression: `scripts/check_fixed_arrays.py` (native, QZI, `--valgrind`).
