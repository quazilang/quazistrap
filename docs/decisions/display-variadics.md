# D-024: Display variadics replace `@format`

Audience: language users and compiler developers.

Status: **decided** by the maintainers (answer В11В, 2026-11-10 and
2026-11-15): no `@format` attribute; variadics work as ordinary language
features and formatting needs no compiler crutch.

## Decision

- A variadic parameter may be declared with a trait type:
  `...args: Display`. A call passes any number of values whose types
  implement `Display`; each arrives in the body as its display text, so the
  parameter is a `[str]` there (`args.len()`, `args[i]`, `for a : args`).
- `...args` as the last call argument forwards a variadic parameter of the
  enclosing function to another variadic call unchanged.
- `format(template, ...args: Display) String` is an ordinary prelude
  function: each `{}` takes the next value, `{{`/`}}` are literal braces,
  and without values the template is returned as it is (a non-literal text
  is never re-parsed for braces).
- `print`, `println`, `err`, `errln` and `Stdout`/`Stderr` `writef`/
  `writelnf` are ordinary Quazi functions that call `format`.
- A literal template passed to a `...: Display` function keeps its rules
  from D-020: `{name}` and `{expr:spec}` are interpolated, `{}` and
  `{:spec}` take the explicit values in order. The call site converts each
  value to text (applying its spec) and passes the template with `{}` slots;
  the function formats at run time.
- `Display.to_string` borrows its receiver (`fn to_string(self: &Self)
  String`). A value of a user type that implements `Display` is shown
  through it; the value stays usable and the text is freed after the call.
- `@format` is an error that points to `...args: Display`. `any` remains
  reserved and is no longer accepted anywhere.

## Consequences

- Formatting functions are written and read like any other function:

  ```quazi
  pub fn log(level: str, template: str, ...args: Display) void {
      const text = format(template, ...args);
      println("[{level}] {}", text.as_str());
  }
  ```

- The `Write` trait keeps `write`/`writeln`; trait methods cannot declare
  variadics yet, so the formatting methods are inherent.
- Calls whose template is not a literal still pass each value's text; the
  callee formats only when values follow.
