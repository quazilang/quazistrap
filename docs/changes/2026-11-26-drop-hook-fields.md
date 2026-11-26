# A `free` hook no longer leaks the fields it leaves

Date: 2026-11-26

Audience: Quazi users who write `free(self: T)` hooks (D-028).

After a struct's `free` hook returns, the compiler now destroys every owned
field the hook did not move out, then releases the struct. Before, such
fields leaked. Hooks that already release each field (`self.name.free();`,
`const file: File = self.file;`) behave as before; that code can now be
removed, but does not have to be.

A method of the type itself that takes `self` by value may now move fields
out of `self`, even when the type has a `free` hook:

```quazi
impl Writer {
    fn free(self: Writer) void { flush_now(&self.inner!); }
    fn into_inner(self: Writer) File { ret self.inner; }   // was S10
}
```

When any field has moved out, the hook does not run for that `self`; the
remaining fields are destroyed one by one. If nothing moved (also on only
some paths), the hook runs as usual.

`std.stream.BufWriter` uses this: dropping it flushes the buffered bytes.

Fixed at the same time (found while reviewing this change; they also
affected structs without hooks):

- `expr?` on its failure path built the returned `Err`/`None` in register 0
  before scope cleanup, overwriting the first parameter: an owned first
  parameter was destroyed as the wrong value (garbage `drop`, double free).
- A field moved out in one `match` arm (`match k { 0 => p.a, _ => ... }`)
  was treated as moved on every arm, so the other arms leaked it (and, for
  a hooked receiver, skipped the hook). The moved bit is now set only when
  the arm that ran produced that field.
- A closure capturing a value after a field moved out of it moved the
  partial value whole (double free). That is now S10 "cannot capture
  partially moved value".
