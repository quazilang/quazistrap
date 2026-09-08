# Chapter 3: Control flow

This chapter covers conditionals, loops, and pattern matching.

## Conditionals

### `if` / `else if` / `else`

```quazi
import std.io;

fn classify(temp: i32) str {
    if (temp < 0) {
        ret "freezing";
    } else if (temp < 20) {
        ret "cool";
    } else if (temp < 35) {
        ret "warm";
    } else {
        ret "hot";
    }
}

fn main() i32 {
    io.println("15°C is {}", classify(15));
    io.println("40°C is {}", classify(40));
    ret 0;
}
```

Conditions are always parenthesized. Braces are required.

## Loops

Quazi uses `for` for all loop forms.

### Range loops

```quazi
// Exclusive upper bound: 0, 1, 2, ..., 9
for i : 0..10 {
    io.println("{}", i);
}

// Inclusive upper bound: 1, 2, 3, 4, 5
for i : 1..=5 {
    io.println("{}", i);
}
```

### Iterator loops

```quazi
var items = [10, 20, 30];
for value : items {
    io.println("Value: {}", value);
}
```

### Index-value loops

```quazi
var names = ["Alice", "Bob", "Carol"];
for index, name : names {
    io.println("{}: {}", index, name);
}
```

### C-style loops

```quazi
for var i = 0; i < 5; i++ {
    io.println("Step {}", i);
}
```

### Condition loops (while-like)

```quazi
var count: i32 = 10;
for (count > 0) {
    io.println("Countdown: {}", count);
    count--;
}
```

### Infinite loops

```quazi
var tries: i32 = 0;
for {
    tries++;
    if (tries >= 3) { break; }
}
io.println("Stopped after {} tries", tries);
```

## `break` and `continue`

`break` exits the nearest loop. `continue` skips to the next iteration:

```quazi
// Print odd numbers up to 20
for i : 1..=20 {
    if (i % 2 == 0) { continue; }
    io.println("{}", i);
}

// Find the first multiple of 7 above 50
for i : 51..100 {
    if (i % 7 == 0) {
        io.println("Found: {}", i);
        break;
    }
}
```

## Pattern matching

`match` tests a value against patterns:

```quazi
const day: i32 = 3;
const name: str = match day {
    1 => "Monday",
    2 => "Tuesday",
    3 => "Wednesday",
    4 => "Thursday",
    5 => "Friday",
    _ => "Weekend",
};
io.println("Day: {}", name);
```

### Enum matching

```quazi
enum Shape {
    Circle(f64),
    Rect(f64, f64),
}

fn area(s: Shape) f64 {
    ret match s {
        Shape.Circle(r) => r * r * 3.14159,
        Shape.Rect(w, h) => w * h,
    };
}
```

### Guards

Patterns can include `if` conditions:

```quazi
fn describe(value: i32) str {
    ret match value {
        0 => "zero",
        n if n > 0 => "positive",
        _ => "negative",
    };
}
```

### Option matching

```quazi
fn safe_divide(a: i32, b: i32) Option[i32] {
    if (b == 0) { ret None; }
    ret Some(a / b);
}

fn main() i32 {
    const result = safe_divide(10, 3);
    match result {
        Some(value) => io.println("Result: {}", value),
        None => io.println("Cannot divide by zero"),
    };
    ret 0;
}
```

Enum matches must be exhaustive — cover every variant or include a `_`
wildcard.

## Next steps

Continue to [Chapter 4: Functions](04-functions.md) to learn about function
definitions, generics, and closures.
