# Chapter 5: Structs, enums, and traits

This chapter covers custom types and polymorphism.

## Structs

Structs group related data into named fields:

```quazi
// tutorial: fragment — requires the surrounding chapter context.
struct Point {
    x: f64,
    y: f64,
}

fn main() i32 {
    const origin = Point { x: 0.0, y: 0.0 };
    const p = Point { x: 3.0, y: 4.0 };
    io.println("({}, {})", p.x, p.y);
    ret 0;
}
```

### Const fields

Fields marked `const` cannot be modified after construction:

```quazi
// tutorial: fragment — requires the surrounding chapter context.
struct Config {
    const name: str,
    var retries: i32,
}
```

### Visibility

`pub` makes a struct accessible outside its module:

```quazi
// tutorial: fragment — requires the surrounding chapter context.
pub struct Color { r: u8, g: u8, b: u8, }
```

## Methods

### Inherent methods

`impl` blocks add methods to a struct:

```quazi
// tutorial: fragment — requires the surrounding chapter context.
struct Rectangle { width: f64, height: f64, }

impl Rectangle {
    // Static method (no self parameter)
    fn square(size: f64) Rectangle {
        ret Rectangle { width: size, height: size };
    }

    // Instance method
    fn area(self: Rectangle) f64 {
        ret self.width * self.height;
    }

    fn perimeter(self: Rectangle) f64 {
        ret 2.0 * (self.width + self.height);
    }
}

fn main() i32 {
    const sq = Rectangle.square(5.0);
    io.println("Area: {}", sq.area());
    io.println("Perimeter: {}", sq.perimeter());
    ret 0;
}
```

Static methods are called as `Type.method(...)`. Instance methods use dot
syntax on a value.

## Enums

Enums define a type with a fixed set of variants:

```quazi
// tutorial: fragment — requires the surrounding chapter context.
enum Direction { North, South, East, West, }

enum Shape {
    Circle(f64),
    Rect(f64, f64),
    Triangle(f64, f64),
}
```

Variants may carry payload values. Construction:

```quazi
// tutorial: fragment — requires the surrounding chapter context.
const dir = Direction.North;
const circle = Shape.Circle(5.0);
const rect = Shape.Rect(4.0, 6.0);
```

### Matching enums

Enum matches must cover every variant:

```quazi
// tutorial: fragment — requires the surrounding chapter context.
fn area(s: Shape) f64 {
    ret match s {
        Shape.Circle(r) => r * r * 3.14159,
        Shape.Rect(w, h) => w * h,
        Shape.Triangle(b, h) => 0.5 * b * h,
    };
}
```

### Generic enums

The prelude provides `Option[T]` and `Result[T, E]`:

```quazi
// tutorial: fragment — requires the surrounding chapter context.
enum Option[T] { Some(T), None, }
enum Result[T, E] { Ok(T), Err(E), }
```

Contextual constructors (`Some`, `None`, `Ok`, `Err`) are available without
qualification when the type is clear from context.

## Traits

Traits declare shared behavior:

```quazi
// tutorial: fragment — requires the surrounding chapter context.
trait Area {
    fn area(self: Self) f64;
}
```

### Implementing traits

```quazi
// tutorial: fragment — requires the surrounding chapter context.
struct Circle { radius: f64, }

impl Area for Circle {
    fn area(self: Circle) f64 {
        ret self.radius * self.radius * 3.14159;
    }
}
```

### Using trait methods

```quazi
// tutorial: fragment — requires the surrounding chapter context.
const c = Circle { radius: 5.0 };
io.println("Area: {}", c.area());
```

### Dynamic dispatch

`dyn Trait` represents any value implementing the trait, dispatched through a
vtable:

```quazi
// tutorial: fragment — requires the surrounding chapter context.
fn print_area(shape: dyn Area) void {
    io.println("Area: {}", shape.area());
}
```

## Generic structs

```quazi
// tutorial: fragment — requires the surrounding chapter context.
struct Pair[A, B] {
    first: A,
    second: B,
}

const p = Pair[str, i32] { first: "age", second: 30 };
```

## Type aliases

```quazi
// tutorial: fragment — requires the surrounding chapter context.
type Rune = u32;
type Coordinate = f64;
```

Aliases are transparent to the type system.

## Derive

`@derive` registers trait implementations:

```quazi
// tutorial: fragment — requires the surrounding chapter context.
@derive(Serialize)
struct User {
    name: String,
    age: i64,
    active: bool,
}
```

Currently `Serialize` generates JSON output for structs with `bool`, `i64`,
and `String` fields.

## Next steps

Continue to [Chapter 6: Modules and packages](06-modules.md) to learn about
organizing code across files.
