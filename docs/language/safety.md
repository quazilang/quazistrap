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

An explicit reference receiver borrows the owner. A bare move-only `self: T`
receiver consumes it at a resolved direct inherent call:

```quazi
const length: usize = text.len();  // `len` declares a shared receiver
```

An implementation may declare a read-only receiver explicitly as
`self: &T`. Such a method is permitted through a shared reference and while
other shared loans of the same value are live. The compiler rejects writes
through that receiver. The receiver pointee must match the enclosing `impl`
target. An explicit `self: &T!` receiver may write through its receiver and
exclusively borrows the owner for the call. A bare move-only receiver transfers
its cleanup responsibility to the callee. A move-only field, indexed element,
or value reached through a safe dereference cannot be moved in any consuming
position yet, including as a function argument or return value. Scalar
projections and whole-owner moves remain valid. Place-level moves, structural
destruction, whole-program, and cross-call effects remain D-014 work.
A consuming `free(self: T)` hook may consume projections rooted in that
receiver while performing its own cleanup, because the complete receiver was
already transferred to the hook.

## Scope cleanup (RAII)

Owned locals are destroyed in reverse lexical order when they go out of scope.
This applies on:

- Normal fallthrough at the end of a block.
- Early return via `ret`.

`String.free()` can release an owned string before scope exit. It leaves the
value in the same empty state as `String.new()`, so a repeated explicit call is
a no-op. A direct `free()` call on a named local suppresses that local's later
automatic cleanup. Any borrowed string view or raw pointer obtained before the
first release is invalid afterward.

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
- The referenced owner cannot be mutated or moved while the borrow exists.
  A read-only method declared with `self: &T` is permitted.
- Fields, indexes, dereferences, calls, and temporaries are not valid
  address-of operands.
- Dereferencing a shared reference works for scalar pointees. Aggregate
  pointees require immutable receiver semantics.
- `str`/`&str` are the representation-identical string-view exception.

`&T!` is an exclusive safe reference. It may currently target only a mutable
local variable or parameter through `&value!`, and assignment through
`*reference` is allowed.
The conservative local checker rejects owner reads, moves, mutation, and any
overlapping shared or exclusive borrow while the loan's lexical scope is live.
For a temporary `&value` or `&value!` argument to a resolved direct Quazi call,
the checker ends the loan after that call; current safe references cannot
escape such a callee.
It does not yet derive shorter use-based regions, support reborrowing, model
flow-sensitive or cross-call effects, or verify QZI-only summaries; those
remain D-014 implementation work.

These restrictions keep references sound while whole-program ownership is
implemented. See
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
