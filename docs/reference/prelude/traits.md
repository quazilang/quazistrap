# `prelude.traits`

prelude.traits — core language traits

## Contents

- [`Display`](#display) (trait)
- [`Debug`](#debug) (trait)
- [`Eq`](#eq) (trait)
- [`Ord`](#ord) (trait)
- [`Clone`](#clone) (trait)
- [`Copy`](#copy) (trait)
- [`Default`](#default) (trait)
- [`Hash`](#hash) (trait)
- [`Iterator`](#iterator) (trait)
- [`Into`](#into) (trait)
- [`From`](#from) (trait)
- [`Drop`](#drop) (trait)
- [`Index`](#index) (trait)
- [`Write`](#write) (trait)
- [`i8`](#i8) (methods)
- [`i16`](#i16) (methods)
- [`i32`](#i32) (methods)
- [`i64`](#i64) (methods)
- [`u8`](#u8) (methods)
- [`u16`](#u16) (methods)
- [`u32`](#u32) (methods)
- [`u64`](#u64) (methods)
- [`isize`](#isize) (methods)
- [`usize`](#usize) (methods)
- [`f16`](#f16) (methods)
- [`f32`](#f32) (methods)
- [`f64`](#f64) (methods)
- [`bool`](#bool) (methods)
- [`str`](#str) (methods)
- [`String`](#string) (methods)
- [`Add`](#add) (trait)
- [`Sub`](#sub) (trait)
- [`Mul`](#mul) (trait)
- [`Div`](#div) (trait)
- [`Rem`](#rem) (trait)
- [`Neg`](#neg) (trait)
- [`BitAnd`](#bitand) (trait)
- [`BitOr`](#bitor) (trait)
- [`BitXor`](#bitxor) (trait)
- [`Shl`](#shl) (trait)
- [`Shr`](#shr) (trait)

## `Display`

```quazi
pub trait Display {
    fn to_string(self: &Self) String;
}
```

── Display ──────────────────────────────────────────────────────────────────
Human-readable text: `to_string()` returns an owned String (maintainer
answer В9В: there is no borrowed `to_str()`; numbers are formatted into
their own buffer, which is safe in every thread).

## `Debug`

```quazi
pub trait Debug {
    fn debug_str() &str;
}
```

── Debug ────────────────────────────────────────────────────────────────────
Programmer-readable representation (includes type info, escapes, etc.).

## `Eq`

```quazi
pub trait Eq[T] {
    fn eq(other: T) bool;
    fn ne(other: T) bool;
}
```

── Eq ───────────────────────────────────────────────────────────────────────

## `Ord`

```quazi
pub trait Ord[T] {
    fn cmp(other: T) i32;
    fn lt(other: T) bool;
    fn le(other: T) bool;
    fn gt(other: T) bool;
    fn ge(other: T) bool;
}
```

── Ord ──────────────────────────────────────────────────────────────────────
cmp returns negative if self < other, 0 if equal, positive if self > other.

## `Clone`

```quazi
pub trait Clone {
    fn clone() Self;
}
```

── Clone ────────────────────────────────────────────────────────────────────

## `Copy`

```quazi
pub trait Copy {
}
```

── Copy ─────────────────────────────────────────────────────────────────────
A value that may be duplicated bit for bit: using it does not move it.
Integers, floats, `bool`, `str`, shared references and raw pointers are
Copy; a struct is Copy through `@derive(Copy)` or `impl Copy for S {}` when
every field is Copy. A generic parameter is Copy only under `T: Copy`.

## `Default`

```quazi
pub trait Default {
    fn default() Self;
}
```

── Default ──────────────────────────────────────────────────────────────────

## `Hash`

```quazi
pub trait Hash {
    fn hash() u64;
}
```

── Hash ─────────────────────────────────────────────────────────────────────

## `Iterator`

```quazi
pub trait Iterator[T] {
    fn next() Option[T];
    fn has_next() bool;
}
```

── Iterator ─────────────────────────────────────────────────────────────────

## `Into`

```quazi
pub trait Into[T] {
    fn into() T;
}
```

── Into / From ───────────────────────────────────────────────────────────────

## `From`

```quazi
pub trait From[T] {
    fn from(val: T) Self;
}
```

Conversion from `T`: `Target.from(value)`.

## `Drop`

```quazi
pub trait Drop {
    fn drop() void;
}
```

── Drop ─────────────────────────────────────────────────────────────────────

## `Index`

```quazi
pub trait Index[I, O] {
    fn index(self: &Self, i: I) O;
}
```

── Index ────────────────────────────────────────────────────────────────────

## `Write`

```quazi
pub trait Write {
    fn write(self: Self, s: str) void;
    fn writeln(self: Self, s: str) void;
}
```

── Write ─────────────────────────────────────────────────────────────────────
Implemented by Stdout and Stderr. Their formatting methods (`writef`,
`writelnf`) take `...args: Display` and are inherent methods.

## `i8`

### `i8.to_string`

```quazi
pub fn to_string(self: &i8) String
```

The decimal text of the value as an owned String.

## `i16`

### `i16.to_string`

```quazi
pub fn to_string(self: &i16) String
```

The decimal text of the value as an owned String.

## `i32`

### `i32.to_string`

```quazi
pub fn to_string(self: &i32) String
```

The decimal text of the value as an owned String.

## `i64`

### `i64.to_string`

```quazi
pub fn to_string(self: &i64) String
```

The decimal text of the value as an owned String.

## `u8`

### `u8.to_string`

```quazi
pub fn to_string(self: &u8) String
```

The decimal text of the value as an owned String.

## `u16`

### `u16.to_string`

```quazi
pub fn to_string(self: &u16) String
```

The decimal text of the value as an owned String.

## `u32`

### `u32.to_string`

```quazi
pub fn to_string(self: &u32) String
```

The decimal text of the value as an owned String.

## `u64`

### `u64.to_string`

```quazi
pub fn to_string(self: &u64) String
```

The decimal text of the value as an owned String.

## `isize`

### `isize.to_string`

```quazi
pub fn to_string(self: &isize) String
```

The decimal text of the value as an owned String.

## `usize`

### `usize.to_string`

```quazi
pub fn to_string(self: &usize) String
```

The decimal text of the value as an owned String.

## `f16`

### `f16.to_string`

```quazi
pub fn to_string(self: &f16) String
```

The decimal text of the value as an owned String.

## `f32`

### `f32.to_string`

```quazi
pub fn to_string(self: &f32) String
```

The decimal text of the value as an owned String.

## `f64`

### `f64.to_string`

```quazi
pub fn to_string(self: &f64) String
```

The decimal text of the value as an owned String.

## `bool`

### `bool.to_string`

```quazi
pub fn to_string(self: &bool) String
```

The decimal text of the value as an owned String.

## `str`

### `str.to_string`

```quazi
pub fn to_string(self: &str) String
```

An owned copy of the text.

## `String`

### `String.to_string`

```quazi
pub fn to_string(self: &String) String
```

An owned copy of the text.

## `Add`

```quazi
pub trait Add[T] { fn add(rhs: T) T; }
```

── Operator overloading ──────────────────────────────────────────────────────

## `Sub`

```quazi
pub trait Sub[T] { fn sub(rhs: T) T; }
```

Overloads `-`.

## `Mul`

```quazi
pub trait Mul[T] { fn mul(rhs: T) T; }
```

Overloads `*`.

## `Div`

```quazi
pub trait Div[T] { fn div(rhs: T) T; }
```

Overloads `/`.

## `Rem`

```quazi
pub trait Rem[T] { fn rem(rhs: T) T; }
```

Overloads `%`.

## `Neg`

```quazi
pub trait Neg    { fn neg() Self; }
```

Overloads unary `-`.

## `BitAnd`

```quazi
pub trait BitAnd[T] { fn bit_and(rhs: T) T; }
```

Overloads `&`.

## `BitOr`

```quazi
pub trait BitOr[T]  { fn bit_or(rhs: T) T; }
```

Overloads `|`.

## `BitXor`

```quazi
pub trait BitXor[T] { fn bit_xor(rhs: T) T; }
```

Overloads `^`.

## `Shl`

```quazi
pub trait Shl[T] { fn shl(n: T) T; }
```

Overloads `<<`.

## `Shr`

```quazi
pub trait Shr[T] { fn shr(n: T) T; }
```

Overloads `>>`.
