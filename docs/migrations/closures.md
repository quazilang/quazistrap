# Migrating Function Values and Closures

Quazi function values now own their closure environment. This closes leaks and
prevents escaping closures from retaining dangling shallow captures, but code
that relied on implicit copying or unsupported capture shapes must change.

## Ownership changes

Passing, returning, or assigning a value of type `fn(...) Return` transfers it.
The previous binding cannot be called afterward. Calling the value itself only
borrows it and remains repeatable.

```quazi
fn run(callback: fn() i32) i32 { ret callback(); }

fn main() void {
    var callback: fn() i32 = || 42;
    var answer: i32 = run(callback);
    // callback(); // error: callback moved into run
}
```

Do not self-assign a function owner. To replace one, assign a newly created
closure or named function value; the compiler destroys the old environment.

## Temporary restrictions

Until recursive destruction is implemented, closure captures, parameters, and
results must have plain scalar runtime shapes: booleans, numeric types, raw
pointers, or C function pointers. Captures are immutable. Quazi `fn` values
cannot be fields, enum payloads, array elements, or generic type arguments.

Move owned state through an explicit non-closure API, or pass the state as a
plain scalar parameter. Keep callback collections and optional callbacks out of
safe APIs for now.

Function signatures are invariant: `fn() i32` does not implicitly become
`fn() f64`. Add an ordinary adapter function or closure with the exact desired
signature and an explicit conversion in its body.

Moving an outer function owner from a conditional branch, loop, short-circuit
operand, or match arm is temporarily rejected because cleanup state is not yet
path-sensitive. Create the owner inside that path, or move it before entering
the control-flow construct. Split a consuming assignment expression into an
assignment statement followed by a move of the binding.

Incremental QZC v4 caches reject older closure chunks automatically. QZI v7 is
the affine function-value ownership boundary. The reader accepts compatible
older bytecode only when it has neither a public owned-function contract nor
legacy closure/forwarder chunks; otherwise rebuild from source. Source
publication is still required for public generic APIs.
