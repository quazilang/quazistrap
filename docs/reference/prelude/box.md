# `prelude.box`

prelude.box — heap-allocated smart pointer Box[T]

## Contents

- [`Box`](#box) (struct)

## `Box`

```quazi
pub struct Box[T] { /* internal fields */ }
```

An owned value of type `T` stored on the heap; the storage is freed with
the box.

### `Box.new`

```quazi
pub fn new(val: T) Box[T]
```

A box holding `val`.

### `Box.get`

```quazi
pub fn get(self: Box[T]) T
```

Moves the value out of the box.

### `Box.free`

```quazi
pub fn free(self: Box[T]) void
```

Frees the storage; called automatically at the end of scope.
