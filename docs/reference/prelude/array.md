# `prelude.array`

prelude.array — safe generic dynamic array backed by heap memory

## Contents

- [`Array`](#array) (struct)

## `Array`

```quazi
pub struct Array[T] { /* internal fields */ }
```

A growable list of values of type `T`, stored contiguously. It frees its
elements and storage when it goes out of scope. `for value : array`
consumes it; `for value : &array` reads copyable elements in place.

### `Array.new`

```quazi
pub fn new() Array[T]
```

An empty array.

### `Array.from`

```quazi
pub fn from(...items: T) Array[T]
```

An array of the given values: `Array.from(1, 2, 3)`.

### `Array.push`

```quazi
pub fn push(self: &Array[T]!, val: T) void
```

Appends `val` at the end, growing the storage when needed.

### `Array.get`

```quazi
pub fn get(self: &Array[T], idx: usize) T
```

A copy of the element at `idx`; panics when `idx >= len()`.

### `Array.set`

```quazi
pub fn set(self: &Array[T]!, idx: usize, val: T) void
```

Replaces the element at `idx` with `val` and drops the old element;
panics when `idx >= len()`. (The compiler lowers this contract through a
checked load, recursive drop, then store.)

### `Array.remove`

```quazi
pub fn remove(self: &Array[T]!, idx: usize) T
```

Removes the element at `idx` and returns it, moving the later elements
down by one; panics when `idx >= len()`. (The compiler transfers the
slot to the caller and compacts the live range.)

### `Array.len`

```quazi
pub fn len(self: &Array[T]) usize
```

The number of elements.

### `Array.is_empty`

```quazi
pub fn is_empty(self: &Array[T]) bool
```

Whether the array has no elements.

### `Array.as_ptr`

```quazi
pub fn as_ptr(self: &Array[T]) *T
```

Exposes the first contiguous element for FFI-style borrowed slices.
The pointer is invalidated by any operation that can reallocate or free
this array. Callers must stay within len elements and preserve valid
initialized values and ownership invariants when writing through it.

### `Array.free`

```quazi
pub fn free(self: Array[T]) void
```

Frees the storage; called automatically at the end of scope.

### `Array.index`

```quazi
pub fn index(self: &Array[T], i: usize) T
```

`array[i]`: the element at `i`; panics when out of range.
