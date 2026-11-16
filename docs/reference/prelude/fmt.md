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
pub fn format(template: str, ...args: Display) String
```

Replaces each `{}` in `template` by the text of the next value; `{{` and
`}}` are literal braces. Without values the template is returned as it is
(a text that is not a literal template is never re-parsed for braces).
Each value arrives as its display text (`...args: Display`, D-024).
