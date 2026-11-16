# `prelude.panic`

prelude.panic — panic and panic handler

Default: prints "thread 'main' panicked at '<msg>'\n  at <file>:<line>\n" to stderr and exits 101.

Override the handler by defining a function with @panic_handler:
  @panic_handler
  fn my_handler(info: PanicInfo) ! { ... }
The compiler replaces the default with your function automatically.

## Contents

- [Functions](#functions)
- [`PanicInfo`](#panicinfo) (struct)

## Functions

### `panic`

```quazi
pub fn panic(msg: str, file: str, line: usize, ...args: Display) !
```

Stops the program with a message. The compiler supplies `file` and `line`;
write `panic("bad value {}", value)`. The panic handler prints
`thread 'main' panicked at '...'` and exits with status 101.

## `PanicInfo`

```quazi
pub struct PanicInfo { /* internal fields */ }
```

What a panic handler receives: the message and the source location of
the `panic`.

### `PanicInfo.message`

```quazi
pub fn message(self: &PanicInfo) str
```

The panic message.

### `PanicInfo.file`

```quazi
pub fn file(self: &PanicInfo) str
```

The source file of the `panic` call.

### `PanicInfo.line`

```quazi
pub fn line(self: &PanicInfo) usize
```

The line of the `panic` call.
