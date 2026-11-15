# `prelude.option`

prelude.option — Option[T] methods
Option[T] is a builtin enum: Some(T) = tag 1, None = tag 0.

## Contents

- [`Option`](#option) (methods)

## `Option`

### `Option.is_some`

```quazi
pub fn is_some(self: &Option[T]) bool
```

Whether this is `Some(value)`.

### `Option.is_none`

```quazi
pub fn is_none(self: &Option[T]) bool
```

Whether this is `None`.

### `Option.ok`

```quazi
pub fn ok(self: &Option[T]) bool
```

Returns true if Some, false if None.

### `Option.unwrap`

```quazi
pub fn unwrap(self: Option[T]) T
```

The value inside `Some`; panics on `None`.

### `Option.unwrap_or`

```quazi
pub fn unwrap_or(self: Option[T], default: T) T
```

The value inside `Some`, or `default` for `None`.

### `Option.or_else`

```quazi
pub fn or_else(self: Option[T], other: Option[T]) Option[T]
```

This option if it is `Some`, otherwise `other`.
