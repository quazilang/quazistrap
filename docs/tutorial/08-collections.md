# Chapter 8: Collections

This chapter covers arrays, maps, and sets.

## Fixed arrays

Fixed arrays have a known size at compile time:

```quazi
const rgb: [u8; 3] = [255, 128, 0];
const first: u8 = rgb[0];     // 255
const count: usize = rgb.len(); // 3 (if applicable)
```

Indexing is bounds-checked in safe code.

## Dynamic arrays

`Array[T]` is the prelude's growable collection:

```quazi
import std.io;

fn main() i32 {
    var numbers: Array[i32] = Array[i32].new();

    // Adding elements
    numbers.push(10);
    numbers.push(20);
    numbers.push(30);

    // Accessing elements
    io.println("First: {}", numbers[0]);
    io.println("Length: {}", numbers.len());

    // Modifying elements
    numbers[0] = 99;

    // Iterating
    for value : numbers {
        io.println("Value: {}", value);
    }

    // Index-value iteration
    for i, value : numbers {
        io.println("[{}] = {}", i, value);
    }

    ret 0;
}
```

### Checked access

`get(index)` returns `Option[T]` instead of panicking:

```quazi
const maybe = numbers.get(10);
match maybe {
    Some(v) => io.println("Found: {}", v),
    None => io.println("Index out of bounds"),
};
```

### Ownership

`Array[T]` is an owned type. It is cleaned up automatically at scope exit.
Passing it to a function may transfer ownership. Use `free()` for explicit
early release if needed.

## Maps

`std.collections` provides an open-addressing hash map with `usize` keys
and values:

```quazi
import std.collections.Map;

fn main() i32 {
    var scores = Map.new();

    // Insert (mutates in place, returns whether a new entry was added)
    scores.insert(1, 100)?;
    scores.insert(2, 85)?;
    scores.insert(3, 92)?;

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
    scores.remove(3)?;

    io.println("Total players: {}", scores.len());
    ret 0;
}
```

### In-place updates

`insert` and `remove` mutate the map directly. They do not return a new map:

```quazi
// Correct:
scores.insert(4, 77)?;

// No longer needed (old pattern):
// scores = scores.insert(4, 77)?;
```

## Sets

`std.collections` provides an open-addressing set with `usize` values:

```quazi
import std.collections.Set;

fn main() i32 {
    var seen = Set.new();

    seen.insert(10)?;
    seen.insert(20)?;
    seen.insert(30)?;

    if (seen.contains(20)) {
        io.println("20 is in the set");
    }

    seen.remove(20)?;
    io.println("Set size: {}", seen.len());
    ret 0;
}
```

## Box

`Box[T]` is a heap-allocated single value:

```quazi
var boxed = Box[i32].new(42);
io.println("Value: {}", boxed.get());
boxed.set(100);
io.println("Updated: {}", boxed.get());
// Automatically freed at scope exit
```

## String as a collection

Strings support collection-style operations:

```quazi
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

// Iteration
for i, ch : text {
    io.println("[{}] = {}", i, ch);
}
```

## Next steps

Continue to [Chapter 9: File and console I/O](09-io.md) to learn about
reading and writing files.
