# Read-only `*T` and writable `*T!` (D-027)

Date: 2026-11-26

Audience: Quazi users of raw pointers (FFI, allocation, `unsafe` code).

A write through `*T` is now an error (S07). Make the pointer writable where
the memory is meant to be written:

```text
var buffer: *u8 = core.malloc(64);      ->  var buffer: *u8! = core.malloc(64);
*(state + 8) as *u32 ...                 ->  *((state + 8) as *u32!) = value;
unsafe fn fill(out: *i32)                ->  unsafe fn fill(out: *i32!)
fill(&value)                             ->  fill(&value!)
const p: *u8 = address;   (an integer)   ->  keep the pointer itself; only 0 converts
```

Reads need no change. A `*T!` is accepted wherever a `*T` is expected; the
reverse needs `as *T!`. A pointer loaded from memory is read-only unless the
cast says otherwise: `*(slot as * *u8!)` yields a `*u8!`.
