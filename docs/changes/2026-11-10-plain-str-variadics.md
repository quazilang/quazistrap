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

Follow-up the same day (review): impl methods used a fifth copy of the old
rule in codegen and now call `is_format_variadic` too. That exposed an older
defect: instance-method calls never packed a variadic tail, so a method
`sum(self: &Bag, ...values: i32)` received its first arguments in place of the
slice (`bag.sum(7)` faulted, `bag.count("x", "y")` returned 2 by accident).
`compile_method_arguments` now packs the tail with the same
`pack_variadic_block` as free functions.

Verification: a semantic test (`count("a {{", "b")` is plain, `count(5)` is
S08) and the interpolation script's `variadic` program (`count`/`first` with
literals, variables and no arguments, and `str`/`i32` variadic methods).
