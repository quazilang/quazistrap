# `prelude.layout`

prelude.layout — compiler-known runtime layout queries

## Contents

- [Functions](#functions)

## Functions

### `size_of`

```quazi
pub fn size_of[T]() usize
```

The size in bytes of a value of type `T` (`size_of[i64]()` is 8).

### `align_of`

```quazi
pub fn align_of[T]() usize
```

The alignment in bytes of type `T`.
