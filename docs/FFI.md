# C Interoperability

Quazi supports x86-64 SysV and Win64 C ABI calls. FFI is explicit: declarations
use C-compatible types/layout, calls are unsafe, and native linking stays under
manifest/CLI control.

```quazi
import std.ffi.{c_char, c_int};

@api("puts") unsafe fn puts(text: *c_char) c_int;

@export("quazi_add")
pub fn add(left: c_int, right: c_int) c_int { ret left + right; }
```

`@api("symbol")` imports functions or mutable globals. Bare `@api` uses the
Quazi name. `@export("symbol")` exposes a `pub` function through a stable C ABI
adapter. `qz header` emits matching C/C++ declarations.

`@repr(C)` supports structs/unions, scalar fields, packed layout, power-of-two
alignment, named integer bitfields, final flexible arrays, and callback aliases:

```quazi
@repr(C) struct Point { x: f64, y: f64, }
@repr(C) type Callback = fn(i32, i32) i32;
@opaque pub struct NativeHandle {}
```

Only exported Quazi functions coerce to raw C callback values. Calling any raw
callback is unsafe. C variadic imports end with bare `...`; Quazi typed
variadics use `...name: Type` and are not C varargs.

`std.ffi.CStr` borrows a foreign NUL-terminated pointer. `CString` owns an
allocated NUL-terminated buffer and validates embedded NUL. Raw ownership,
lifetime, alignment, callback validity, and mutable global synchronization stay
caller responsibilities.

Native inputs use `[cc]`/`[link]` or CLI object/library flags. Ordinary Quazi
programs use built-in ELF/PE linkers; archives, shared libraries, and arbitrary
native dependencies may select external tools. See [LINKER.md](LINKER.md).
