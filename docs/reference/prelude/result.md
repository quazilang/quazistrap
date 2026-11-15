# `prelude.result`

prelude.result — Result[T, E] methods
Result[T, E] is a builtin enum: Ok(T) = tag 1, Err(E) = tag 0.

## Contents

- [`Result`](#result) (methods)

## `Result`

### `Result.is_ok`

```quazi
pub fn is_ok(self: &Result[T, E]) bool
```

Whether this is `Ok(value)`.

### `Result.is_err`

```quazi
pub fn is_err(self: &Result[T, E]) bool
```

Whether this is `Err(error)`.

### `Result.ok`

```quazi
pub fn ok(self: &Result[T, E]) bool
```

Returns true if Ok, false if Err.

### `Result.unwrap`

```quazi
pub fn unwrap(self: Result[T, E]) T
```

The value inside `Ok`; panics on `Err`.

### `Result.unwrap_err`

```quazi
pub fn unwrap_err(self: Result[T, E]) E
```

The error inside `Err`; panics on `Ok`.

### `Result.unwrap_or`

```quazi
pub fn unwrap_or(self: Result[T, E], default: T) T
```

The value inside `Ok`, or `default` for `Err`.

### `Result.unwrap_err_or`

```quazi
pub fn unwrap_err_or(self: Result[T, E], default: E) E
```

The error inside `Err`, or `default` for `Ok`.
