# `...values: str` without a template is an ordinary variadic

Audience: language users and compiler developers.

Every function whose final parameter was `...values: str` (or a reference)
was treated as a format function: the first string literal argument became a
template, the other arguments were display-converted, and codegen called the
function with one formatted string plus an empty variadic slice. For a
function without a fixed parameter that call did not match its signature:
`count("x", "y")` printed `x` instead of returning 2, and `first(s)` panicked
with an out-of-bounds index.

`parser::format::is_format_variadic` now holds the one rule, used by
declaration, type checking and codegen (previously four copies): a function
follows format-call rules when it is `@format`, or when its final
`...args: str` follows a fixed `str` parameter (as in `fmt.format(template,
...args)` and `panic(msg, file, line, ...args)`). A `...values: str` without
such a parameter is a typed variadic like any other: arguments must be `str`
and no literal is a template. Whether a fixed `str` parameter alone should keep
making a function a format function is open as question В11 in `asks.txt`.

Verification: a semantic test (`count("a {{", "b")` is plain, `count(5)` is
S08) and the interpolation script's `variadic` program (`count`/`first` with
literals, variables and no arguments).
