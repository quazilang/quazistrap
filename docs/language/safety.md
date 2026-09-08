# Safety and ownership

Audience: language users and implementers.

## Unsafe system

Quazi separates safe and unsafe code at the function and block level.

### Unsafe functions

A function with raw pointer parameters (`*T`) must be declared `unsafe fn`.
`@syscall` and `@api` functions are implicitly unsafe.

```quazi
unsafe fn read_raw(pointer: *u8) u8 {
    ret *pointer;
}
```

### Unsafe contexts

The following operations require an `unsafe` block or `unsafe fn`:

- Calling an `unsafe fn`.
- Dereferencing a raw pointer (`*T`).
- Calling an `@api` or `@syscall` function.
- Reading or writing a mutable `@api` foreign global.
- Invoking a raw C callback (`@repr(C)` function pointer).

```quazi
unsafe {
    const byte: u8 = *pointer;
    puts(message);
}
```

An `unsafe` block does not make surrounding code unsafe. It grants permission
for specific unsafe operations within its braces.

### `@intrinsic` functions

`@intrinsic` functions are safe wrappers around compiler-internal operations.
Their unsafety is handled internally; callers do not need `unsafe`.

## Ownership model

Quazi uses value semantics with compiler-managed lexical destruction.

### Copy types

Primitive scalars (`bool`, integers, floats, raw pointers, C function
pointers) are implicitly copied on assignment and parameter passing.

### Move types

Owned types — `String`, `Array[T]`, `Box[T]`, OS handles, `fn` values —
transfer ownership on assignment, parameter passing, or return. The previous
binding becomes invalid:

```quazi
var a: String = String.from("hello");
var b: String = a;  // ownership moves to b
// a is no longer valid here
```

Use-after-move is a compile-time error.

### Borrowed method receivers

Ordinary method calls borrow the receiver. The method does not consume
ownership unless the API explicitly documents it:

```quazi
const length: usize = text.len();  // text is borrowed, not consumed
```

## Scope cleanup (RAII)

Owned locals are destroyed in reverse lexical order when they go out of scope.
This applies on:

- Normal fallthrough at the end of a block.
- Early return via `ret`.

Assignment to a previously initialized owned variable destroys the old value
before storing the new one. Returning an owned value transfers it to the
caller, skipping destruction.

```quazi
fn example() void {
    var s: String = String.from("hello");
    var t: String = String.from("world");
    // t is destroyed first, then s, at scope exit
}
```

### `free()` methods

`free()` is an idempotent early-release operation on resource-owning types.
Normal application code should rely on scope cleanup. Foreign pointers and
borrowed `CStr` never gain ownership automatically.

## Safe references

`&T` is a shared safe reference with conservative lexical restrictions:

- `&value` accepts only a local variable or parameter (parentheses are fine).
- A value never converts into a reference. Pointee types are invariant:
  `&i32` is not `&u64`.
- A shared-reference binding cannot be rebound, returned, stored in an owned
  aggregate, or captured by a closure.
- The referenced owner cannot be mutated, moved, or passed to a method while
  the borrow exists.
- Fields, indexes, dereferences, calls, and temporaries are not valid
  address-of operands.
- Dereferencing a shared reference works for scalar pointees. Aggregate
  pointees require immutable receiver semantics.
- `str`/`&str` are the representation-identical string-view exception.

These restrictions keep references sound before lifetime parameters and
mutable-reference syntax exist. See
[reference migration](../migrations/references.md).

## Raw pointers

`*T` is a raw native pointer. Properties:

- May be null, dangling, misaligned, or point to invalid storage.
- Integer `0` is the null-pointer constant for any `*T`.
- All raw pointer types are mutually compatible: `*T` converts to `*U`.
- Dereferencing requires an `unsafe` context.
- A `&local` may be stored or passed as an exact raw pointer, but
  dereferencing that pointer still requires unsafe context.
- Raw pointers never convert back into safe references.

## Function value ownership

`fn` values own a heap-allocated closure environment:

- Assigning, passing, or returning a `fn` value moves ownership.
- Calling a `fn` value only borrows the environment (repeatable).
- Replacing a binding destroys its previous environment.
- The last owner is destroyed at scope exit.

Current restrictions:

- Function values cannot be placed inside arrays, structs, enums, or generic
  type arguments (no recursive destruction yet).
- Closure captures are limited to immutable plain scalars.
- An outer `fn` owner cannot be moved from only one branch, loop iteration,
  short-circuit operand, or match arm.
- Function-valued assignments are not themselves transferable.
- Match expressions cannot currently produce `fn` values.

`@repr(C)` function-pointer aliases are non-owning raw code pointers and do
not follow these ownership rules.

## Error handling model

Quazi uses `Result[T, E]` for expected failures and `panic` for unrecoverable
bugs. Panics terminate the process; there is no unwinding or recovery.

- `expression?` unwraps `Ok`/`Some`; propagates `Err`/`None` by early return.
- Fallible public APIs return typed error enums (`FsError`, `NetError`,
  `ParseError`, `ReadError`).
- `panic(message)` terminates the process. See [panic handling](panic.md) for
  custom handlers.
