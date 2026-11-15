# `String.push_str` appends in place

Date: 2026-11-15

Audience: Quazi users.

`push_str` returned a new `String` and left the receiver unchanged. A call
whose result was discarded did nothing and gave no warning:

```quazi,ignore
var text = String.new();
text.push_str("abc"); // text stayed empty
```

`push_str(self: &String!, s: str) void` now appends to the receiver, like
Rust's `String::push_str`. The old receiver buffer is released.

Migration: `text = text.push_str(s);` is now a type error (`void` is not a
`String`); write `text.push_str(s);`. Where the appended value is the result,
append and then use the variable (`text.push_str(s); text`).
`String.concat(a, b)` and `fmt.concat(a, b)` still return a new `String`.

Ownership: a `&String!` argument cannot acquire the provenance of other
arguments, because a String outside its declaring file is never built
around a borrowed view (S14); retention analysis no longer assumes it does.

Verification: std `qz test`, `cargo test`, and a 400000-iteration append loop
whose peak memory equals the loop without appends.
