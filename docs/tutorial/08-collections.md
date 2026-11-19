# Chapter 8: Collections

This chapter covers arrays, maps, and sets.

## Fixed arrays

Fixed arrays have a known size at compile time:

```quazi
// tutorial: fragment — requires the surrounding chapter context.
const rgb: [u8; 3] = [255, 128, 0];
const first: u8 = rgb[0];     // 255
const count: usize = rgb.len(); // 3 (if applicable)
```

Indexing is bounds-checked in safe code.

## Dynamic arrays

`Array[T]` is the prelude's growable collection:

```quazi
// tutorial: runnable
import std.io;

fn main() i32 {
    var numbers: Array[i32] = Array.new();

    // Adding elements
    numbers.push(10);
    numbers.push(20);
    numbers.push(30);

    // Accessing elements
    io.println("First: {}", numbers[0]);
    io.println("Length: {}", numbers.len());

    // Modifying elements
    numbers[0] = 99;

    // Iterate by index (`for v : numbers` also works, but consumes the array).
    for i : 0..numbers.len() {
        io.println("[{}] = {}", i, numbers[i]);
    }

    ret 0;
}
```

### Checked access

`get(index)` and `numbers[index]` panic with `index out of bounds` past the
end, so check the length first; `get_ref(index)` borrows an element instead of
copying it:

```quazi
// tutorial: fragment — requires the surrounding chapter context.
const index: usize = 10;
if (index < numbers.len()) {
    io.println("Found: {}", numbers.get(index));
} else {
    io.println("Index out of bounds");
}
```

### Ownership

`Array[T]` is an owned type. It is cleaned up automatically at scope exit.
Passing it to a function may transfer ownership. Use `free()` for explicit
early release if needed.

## Maps

`std.collections` provides `HashMap[K, V]` for any hashable key (for example
`HashMap[String, i32]`) and the specialised open-addressing `Map` with `usize`
keys and values shown here:

```quazi
// tutorial: runnable
import std.collections.Map;
import std.io;

fn main() i32 {
    var scores = Map.new().unwrap();

    // Insert (mutates in place, returns whether a new entry was added)
    scores.insert(1, 100).unwrap();
    scores.insert(2, 85).unwrap();
    scores.insert(3, 92).unwrap();

    // Lookup
    match scores.get(2) {
        Some(score) => io.println("Player 2: {}", score),
        None => io.println("Not found"),
    };

    // Check membership
    if (scores.contains(1)) {
        io.println("Player 1 exists");
    }

    // Remove (mutates in place)
    scores.remove(3);

    io.println("Total players: {}", scores.len());
    ret 0;
}
```

### In-place updates

`insert` and `remove` mutate the map directly. They do not return a new map:

```quazi
// tutorial: fragment — requires the surrounding chapter context.
// Correct:
scores.insert(4, 77).unwrap();

// No longer needed (old pattern):
// scores = scores.insert(4, 77)?;
```

## Sets

`std.collections` provides `HashSet[T]` for any hashable element and the
specialised open-addressing `Set` with `usize` values shown here:

```quazi
// tutorial: runnable
import std.collections.Set;
import std.io;

fn main() i32 {
    var seen = Set.new().unwrap();

    seen.insert(10).unwrap();
    seen.insert(20).unwrap();
    seen.insert(30).unwrap();

    if (seen.contains(20)) {
        io.println("20 is in the set");
    }

    seen.remove(20);
    io.println("Set size: {}", seen.len());
    ret 0;
}
```

## Box

`Box[T]` is a heap-allocated single value. `get` consumes the box and moves
the value out; a box that is never unwrapped is freed at scope exit:

```quazi
// tutorial: fragment — requires the surrounding chapter context.
const boxed: Box[i32] = Box.new(42);
const value: i32 = boxed.get();
io.println("Value: {}", value);
```

## String as a collection

Strings support collection-style operations:

```quazi
// tutorial: fragment — requires the surrounding chapter context.
const text: str = "Hello, Quazi!";

// Length
io.println("Scalars: {}", text.len());
io.println("Bytes: {}", text.bytes_len());

// Indexing (returns Rune)
const first = text[0]; // 'H'

// Slicing
const hello: String = text[0:5]; // "Hello"

// Search
if (text.contains("Quazi")) {
    io.println("Found it!");
}

// Iteration by scalar index (`str` itself is not iterable)
for i : 0..text.len() {
    io.println("[{}] = {}", i, text[i]);
}
```

## Next steps

Continue to [Chapter 9: File and console I/O](09-io.md) to learn about
reading and writing files.
