# `prelude.string`

Owned and borrowed UTF-8 string APIs. User-facing indexes and lengths count
Unicode scalar values. Explicit byte views expose encoded UTF-8 bytes.

## Contents

- [`Rune`](#rune) (type alias)
- [`ParseError`](#parseerror) (enum)
- [`String`](#string) (struct)
- [`str`](#str) (methods)

## `Rune`

```quazi
pub type Rune = u32;
```

One Unicode scalar value (a code point that is not a surrogate).

## `ParseError`

```quazi
pub enum ParseError {
    Empty,
    InvalidDigit(usize),
    Overflow,
}
```

Why `parse_*` rejected its text: `Empty`, `InvalidDigit(index)` (the byte
offset of the first bad character) or `Overflow`.

### `ParseError.message`

```quazi
pub fn message(self: &ParseError) str
```

A short description of the failure.

## `String`

```quazi
pub struct String { /* internal fields */ }
```

Owned, growable UTF-8 text. It frees its buffer when it goes out of scope.
Borrow it as `str` with `as_str()`; append with `push_str`.

### `String.new`

```quazi
pub fn new() String
```

An empty String.

### `String.from`

```quazi
pub fn from(s: str) String
```

A String holding a copy of `s`.

### `String.from_raw`

```quazi
pub fn from_raw(data: *u8, len: usize, cap: usize) String
```

Takes ownership of a writable NUL-terminated UTF-8 allocation.

### `String.len`

```quazi
pub fn len(self: &String) usize
```

The number of Unicode scalar values (runes), not bytes; see `bytes_len`.

### `String.rune_len`

```quazi
pub fn rune_len(self: &String) usize
```

The number of Unicode scalar values (runes).

### `String.bytes_len`

```quazi
pub fn bytes_len(self: &String) usize
```

The length in UTF-8 bytes.

### `String.is_empty`

```quazi
pub fn is_empty(self: &String) bool
```

Whether the text has no characters.

### `String.as_str`

```quazi
pub fn as_str(self: &String) str
```

The text as a borrowed `str` view, valid while the String is unchanged.

### `String.push_str`

```quazi
pub fn push_str(self: &String!, s: str) void
```

Append `s` in place (like Rust's `String::push_str`). It returns
nothing: `text.push_str("x");` changes `text`.

### `String.concat`

```quazi
pub fn concat(a: String, b: String) String
```

A new String: `a` followed by `b`.

### `String.get`

```quazi
pub fn get(self: &String, index: isize) Option[Rune]
```

The rune at `index` (negative counts from the end), or `None` when out
of range.

### `String.reverse`

```quazi
pub fn reverse(self: &String) String
```

A new String with the runes in reverse order.

### `String.to_lowercase`

```quazi
pub fn to_lowercase(self: &String) String
```

A new String with ASCII letters in lower case (other text unchanged).

### `String.to_uppercase`

```quazi
pub fn to_uppercase(self: &String) String
```

A new String with ASCII letters in upper case (other text unchanged).

### `String.lowercase`

```quazi
pub fn lowercase(self: &String) String
```

Same as `to_lowercase`.

### `String.uppercase`

```quazi
pub fn uppercase(self: &String) String
```

Same as `to_uppercase`.

### `String.capitalize`

```quazi
pub fn capitalize(self: &String) String
```

A new String whose first ASCII letter is upper case.

### `String.trim`

```quazi
pub fn trim(self: &String) String
```

A new String without leading and trailing ASCII whitespace.

### `String.starts_with`

```quazi
pub fn starts_with(self: &String, prefix: str) bool
```

Whether the text begins with `prefix`.

### `String.ends_with`

```quazi
pub fn ends_with(self: &String, suffix: str) bool
```

Whether the text ends with `suffix`.

### `String.find`

```quazi
pub fn find(self: &String, needle: str) Option[usize]
```

The UTF-8 byte offset of the first occurrence of `needle`, or `None`.

### `String.contains`

```quazi
pub fn contains(self: &String, needle: str) bool
```

Whether `needle` occurs in the text.

### `String.parse_i64`

```quazi
pub fn parse_i64(self: &String) Result[i64, ParseError]
```

Parses decimal text (optional `+` or `-`) as `i64`. Errors: `Empty`,
`InvalidDigit(index)`, `Overflow` when the value does not fit.

### `String.parse_i8`

```quazi
pub fn parse_i8(self: &String) Result[i8, ParseError]
```

Parses decimal text (optional `+` or `-`) as `i8`. Errors: `Empty`,
`InvalidDigit(index)`, `Overflow` when the value does not fit.

### `String.parse_i16`

```quazi
pub fn parse_i16(self: &String) Result[i16, ParseError]
```

Parses decimal text (optional `+` or `-`) as `i16`. Errors: `Empty`,
`InvalidDigit(index)`, `Overflow` when the value does not fit.

### `String.parse_i32`

```quazi
pub fn parse_i32(self: &String) Result[i32, ParseError]
```

Parses decimal text (optional `+` or `-`) as `i32`. Errors: `Empty`,
`InvalidDigit(index)`, `Overflow` when the value does not fit.

### `String.parse_isize`

```quazi
pub fn parse_isize(self: &String) Result[isize, ParseError]
```

Parses decimal text (optional `+` or `-`) as `isize`. Errors: `Empty`,
`InvalidDigit(index)`, `Overflow` when the value does not fit.

### `String.parse_u8`

```quazi
pub fn parse_u8(self: &String) Result[u8, ParseError]
```

Parses decimal text (optional `+`) as `u8`. Errors: `Empty`,
`InvalidDigit(index)`, `Overflow` when the value does not fit.

### `String.parse_u16`

```quazi
pub fn parse_u16(self: &String) Result[u16, ParseError]
```

Parses decimal text (optional `+`) as `u16`. Errors: `Empty`,
`InvalidDigit(index)`, `Overflow` when the value does not fit.

### `String.parse_u32`

```quazi
pub fn parse_u32(self: &String) Result[u32, ParseError]
```

Parses decimal text (optional `+`) as `u32`. Errors: `Empty`,
`InvalidDigit(index)`, `Overflow` when the value does not fit.

### `String.parse_u64`

```quazi
pub fn parse_u64(self: &String) Result[u64, ParseError]
```

Parses decimal text (optional `+`) as `u64`. Errors: `Empty`,
`InvalidDigit(index)`, `Overflow` when the value does not fit.

### `String.parse_usize`

```quazi
pub fn parse_usize(self: &String) Result[usize, ParseError]
```

Parses decimal text (optional `+`) as `usize`. Errors: `Empty`,
`InvalidDigit(index)`, `Overflow` when the value does not fit.

### `String.parse_f32`

```quazi
pub fn parse_f32(self: &String) Result[f32, ParseError]
```

Parses decimal floating-point text as `f32`.

### `String.parse_f64`

```quazi
pub fn parse_f64(self: &String) Result[f64, ParseError]
```

Parses decimal floating-point text such as `-1.5` or `2.5e10` as `f64`.

### `String.clear`

```quazi
pub fn clear(self: &String!) void
```

Release the owned allocation while retaining a valid, reusable String.
This is the in-place counterpart to consuming free(self).

### `String.free`

```quazi
pub fn free(self: String) void
```

Frees the buffer; called automatically at the end of scope.

### `String.index`

```quazi
pub fn index(self: &String, index: isize) Rune
```

`text[i]`: the rune at `index` (negative counts from the end); panics
when out of range.

## `str`

### `str.len`

```quazi
pub fn len(self: str) usize
```

The number of Unicode scalar values (runes), not bytes; see `bytes_len`.

### `str.rune_len`

```quazi
pub fn rune_len(self: str) usize
```

The number of Unicode scalar values (runes).

### `str.bytes_len`

```quazi
pub fn bytes_len(self: str) usize
```

The length in UTF-8 bytes.

### `str.is_empty`

```quazi
pub fn is_empty(self: str) bool
```

Whether the text has no characters.

### `str.get`

```quazi
pub fn get(self: str, index: isize) Option[Rune]
```

The rune at `index` (negative counts from the end), or `None` when out
of range.

### `str.reverse`

```quazi
pub fn reverse(self: str) String
```

A new String with the runes in reverse order.

### `str.to_lowercase`

```quazi
pub fn to_lowercase(self: str) String
```

Case conversion currently covers ASCII and preserves all non-ASCII runes.

### `str.to_uppercase`

```quazi
pub fn to_uppercase(self: str) String
```

A new String with ASCII letters in upper case (other text unchanged).

### `str.lowercase`

```quazi
pub fn lowercase(self: str) String
```

Same as `to_lowercase`.

### `str.uppercase`

```quazi
pub fn uppercase(self: str) String
```

Same as `to_uppercase`.

### `str.capitalize`

```quazi
pub fn capitalize(self: str) String
```

A new String whose first ASCII letter is upper case.

### `str.trim`

```quazi
pub fn trim(self: str) String
```

A new String without leading and trailing ASCII whitespace.

### `str.starts_with`

```quazi
pub fn starts_with(self: str, prefix: str) bool
```

Whether the text begins with `prefix`.

### `str.ends_with`

```quazi
pub fn ends_with(self: str, suffix: str) bool
```

Whether the text ends with `suffix`.

### `str.find`

```quazi
pub fn find(self: str, needle: str) Option[usize]
```

Like Rust, find() returns a UTF-8 byte offset.

### `str.contains`

```quazi
pub fn contains(self: str, needle: str) bool
```

Whether `needle` occurs in the text.

### `str.parse_i64`

```quazi
pub fn parse_i64(self: str) Result[i64, ParseError]
```

Parses decimal text (optional `+` or `-`) as `i64`. Errors: `Empty`,
`InvalidDigit(index)`, `Overflow` when the value does not fit.

### `str.parse_i32`

```quazi
pub fn parse_i32(self: str) Result[i32, ParseError]
```

Parses decimal text (optional `+` or `-`) as `i32`. Errors: `Empty`,
`InvalidDigit(index)`, `Overflow` when the value does not fit.

### `str.parse_i8`

```quazi
pub fn parse_i8(self: str) Result[i8, ParseError]
```

Parses decimal text (optional `+` or `-`) as `i8`. Errors: `Empty`,
`InvalidDigit(index)`, `Overflow` when the value does not fit.

### `str.parse_i16`

```quazi
pub fn parse_i16(self: str) Result[i16, ParseError]
```

Parses decimal text (optional `+` or `-`) as `i16`. Errors: `Empty`,
`InvalidDigit(index)`, `Overflow` when the value does not fit.

### `str.parse_isize`

```quazi
pub fn parse_isize(self: str) Result[isize, ParseError]
```

Parses decimal text (optional `+` or `-`) as `isize`. Errors: `Empty`,
`InvalidDigit(index)`, `Overflow` when the value does not fit.

### `str.parse_u64`

```quazi
pub fn parse_u64(self: str) Result[u64, ParseError]
```

Parses decimal text (optional `+`) as `u64`. Errors: `Empty`,
`InvalidDigit(index)`, `Overflow` when the value does not fit.

### `str.parse_u8`

```quazi
pub fn parse_u8(self: str) Result[u8, ParseError]
```

Parses decimal text (optional `+`) as `u8`. Errors: `Empty`,
`InvalidDigit(index)`, `Overflow` when the value does not fit.

### `str.parse_u16`

```quazi
pub fn parse_u16(self: str) Result[u16, ParseError]
```

Parses decimal text (optional `+`) as `u16`. Errors: `Empty`,
`InvalidDigit(index)`, `Overflow` when the value does not fit.

### `str.parse_u32`

```quazi
pub fn parse_u32(self: str) Result[u32, ParseError]
```

Parses decimal text (optional `+`) as `u32`. Errors: `Empty`,
`InvalidDigit(index)`, `Overflow` when the value does not fit.

### `str.parse_usize`

```quazi
pub fn parse_usize(self: str) Result[usize, ParseError]
```

Parses decimal text (optional `+`) as `usize`. Errors: `Empty`,
`InvalidDigit(index)`, `Overflow` when the value does not fit.

### `str.parse_f64`

```quazi
pub fn parse_f64(self: str) Result[f64, ParseError]
```

Parses decimal floating-point text such as `-1.5` or `2.5e10` as `f64`.

### `str.parse_f32`

```quazi
pub fn parse_f32(self: str) Result[f32, ParseError]
```

Parses decimal floating-point text as `f32`.

### `str.index`

```quazi
pub fn index(self: &str, index: isize) Rune
```

`text[i]`: the rune at `index` (negative counts from the end); panics
when out of range.
