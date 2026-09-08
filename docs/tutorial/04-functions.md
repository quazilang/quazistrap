# Chapter 4: Functions

This chapter covers function definitions, parameters, return values, generics,
and closures.

## Basic functions

```quazi
fn add(a: i32, b: i32) i32 {
    ret a + b;
}

fn greet(name: str) void {
    io.println("Hello, {}!", name);
}
```

Parameters are typed. Functions return a value with `ret expression;` or
return nothing with `ret;` from a `void` function. Every non-void code path
must reach a `ret`.

## Named arguments

Positional arguments must come before named arguments:

```quazi
fn create_user(name: str, age: i32, active: bool) void { ... }

// All positional:
create_user("Alice", 30, true);

// With named arguments:
create_user("Alice", active=true, age=30);
```

## Variadic functions

The final parameter can accept multiple values:

```quazi
fn sum(first: i32, ...rest: i32) i32 {
    var total: i32 = first;
    for value : rest {
        total += value;
    }
    ret total;
}

const result = sum(1, 2, 3, 4, 5); // 15
```

## Recursion

Functions can call themselves. A terminating base case is your
responsibility:

```quazi
fn factorial(n: i64) i64 {
    if (n <= 1) { ret 1; }
    ret n * factorial(n - 1);
}
```

## Generic functions

Type parameters let you write functions that work with any type:

```quazi
fn first[T](items: Array[T]) T {
    ret items[0];
}

fn swap[T](a: T, b: T) Array[T] {
    var result = Array[T].new();
    result.push(b);
    result.push(a);
    ret result;
}
```

Generic calls normally infer type arguments. Use explicit arguments when
inference is ambiguous:

```quazi
const value = first[i32](numbers);
```

## Closures

Closure expressions create function values:

```quazi
var double: fn(i32) i32 = |x| x * 2;
var add: fn(i32, i32) i32 = |a, b| a + b;

io.println("double(5) = {}", double(5));
io.println("add(3, 4) = {}", add(3, 4));
```

Parameters are inferred from the expected function type. A standalone closure
binding needs an explicit type annotation.

### Passing closures as arguments

```quazi
fn apply(value: i32, f: fn(i32) i32) i32 {
    ret f(value);
}

const result = apply(5, |x| x * x);
io.println("5 squared = {}", result);
```

### Function names as values

A named function can be used as a function value:

```quazi
fn triple(x: i32) i32 { ret x * 3; }

var f: fn(i32) i32 = triple;
io.println("f(4) = {}", f(4));
```

### Ownership

`fn` values own their closure environment. Assigning or passing one moves
ownership. Calling only borrows it, so it can be called repeatedly:

```quazi
var f: fn(i32) i32 = |x| x + 1;
io.println("{}", f(1)); // ok: calling borrows
io.println("{}", f(2)); // ok: still valid

var g = f;  // ownership moves to g
// f is no longer valid here
```

### Current limitations

- Captures are limited to immutable plain scalars.
- Function values cannot be placed inside collections or structs.
- Parameter and return types must match exactly.

## Next steps

Continue to [Chapter 5: Structs, enums, and traits](05-types.md) to learn
about custom types.
