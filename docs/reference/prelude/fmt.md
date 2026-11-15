# `prelude.fmt`

prelude.fmt — owned UTF-8 formatting results

## Contents

- [Functions](#functions)

## Functions

### `concat`

```quazi
pub fn concat(a: str, b: str) String
```

A new String: `a` followed by `b`.

### `format`

```quazi
pub fn format(template: str, ...args: str) String
```

Call-site coercion supplies borrowed display views. Copy each view while it
remains alive and transfer the final owner instead of detaching its buffer.
