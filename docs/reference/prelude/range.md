# `prelude.range`

Integer range produced by `start..end` and `start..=end` expressions.

## Contents

- [`Range`](#range) (struct)

## `Range`

```quazi
pub struct Range {
    start: i64,
    end: i64,
    inclusive: bool,
}
```

Integer range produced by `start..end` and `start..=end` expressions.
