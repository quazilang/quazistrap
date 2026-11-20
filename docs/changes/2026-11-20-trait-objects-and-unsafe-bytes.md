# Trait objects own their values; `core.str_byte_at` is unsafe

Date: 2026-11-20

Audience: Quazi users of `dyn Trait`, QZI libraries and `std.core`.

**Trait objects.** A `dyn Trait` value now owns the concrete value inside it:
destroying the trait object runs the value's `free` hook and releases its
owned fields (before, they leaked). A concrete value becomes a trait object
wherever `dyn Trait` is expected (arguments, returned values, fields, enum
payloads, `Array[dyn T]`, `Box[dyn T]`), not only in declarations; before,
those sites misread the value and crashed. A `dyn Trait` where a concrete
type is expected is S01, and calling a by-value (`self: Self`) method through
`dyn` moves the trait object. Trait objects grew a destroy word, so QZI files
are format 18: rebuild format 17 libraries from source.

**`core.str_byte_at`.** It reads without a bounds check, so it is now an
`unsafe fn` (a too-large index read outside the string in safe code). Wrap
calls whose index is known to be below `text.bytes_len()`:

```text
const b = core.str_byte_at(text, i);
                ->
var b: u8 = 0;
unsafe { b = core.str_byte_at(text, i); }
```
