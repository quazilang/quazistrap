# Struct destructuring in match patterns

Audience: language users and compiler developers.

`match` arms can destructure structs:

```quazi
match p {
    Point { x: 0, y: 0 } => "origin",
    Point { x: 0, y } => "on the y axis",
    Point { x, .. } => "elsewhere",
}
```

- Parser: `PatternKind::Struct { name, fields, rest }`. A field without `:`
  binds a variable of the same name; `field: pattern` takes any pattern
  (literals, nested structs, variants); `..` ignores the other fields.
- Analysis: the pattern's struct must be the scrutinee's struct type (also
  through generic arguments and inside variant payloads such as
  `Some(Point { x, y })`). Unknown fields are S04; a pattern without `..`
  that omits fields is S09; a different struct is S09; `@repr(C)` structs
  are S14 for now. Bindings get the field types. The pattern span is
  annotated with the struct type, which lowering uses for field offsets.
  Like literal arms, refutable struct arms are checked at run time.
- Codegen loads each non-wildcard field and matches it recursively.
- Tree-sitter: `struct_pattern`, `field_pattern`, `rest_pattern`.

Verification: `scripts/check_struct_patterns.py` (literal and shorthand
fields, nested structs, `..`, generic structs, structs in `Option` payloads,
borrowed scrutinees, owned `String` fields with and without a `free` hook,
natively and from source-removed QZI, and the three diagnostics); a parser
regression.
