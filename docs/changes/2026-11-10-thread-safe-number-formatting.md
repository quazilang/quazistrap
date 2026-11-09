# Numbers in format arguments get private digit buffers

Audience: language users and compiler developers.

`PrimToStr` (number or bool to text) wrote its digits into one static
`__quazi_itoa_*` buffer per instruction site. Two threads (or qoroutines on
different workers) running the same `println`, `format` or interpolated
literal overwrote each other's digits: 4 threads formatting 100000 numbers
each and parsing them back saw about 310000 mismatches per run.

Inside a format call (`fmt.format`, `println`/`print` wrappers, interpolated
literals and `@format` instance methods) codegen now allocates an 80-byte heap buffer per numeric argument, passes it to
`PrimToStr` in `ops[3]` with `PRIM_TO_STR_BUFFER_FLAG`, and frees it after the
call returns; the call's result is an owned `String` that no longer refers to
the buffer. Register allocation and remapping treat `ops[3]` as a use when the
flag is set. Outside format calls `PrimToStr` keeps the static buffer, so a
direct `n.to_str()` view is still per-site shared state: the API decision is
open as question В9 in the workspace `asks.txt`, and the format path is safe
under either answer.

Follow-up the same day (independent review):

- Format arguments are evaluated before any is converted, like Rust's
  `format_args!`, so a `?` or branch inside a later argument can no longer
  skip the release of an earlier argument's buffer (it leaked 80 bytes per
  early return). User `...xs: str` functions and variadic intrinsics may return
  one of their argument views, so conversions for them keep the static buffer
  and never join an enclosing format call's buffers (which could otherwise be
  freed on a path that never allocated them). A single numeric argument to a
  format wrapper is formatted like `"{}"` into an owned String.
- Placeholder expressions now get owner sites, so the propagated `Result` of
  `{get()?}` is released (32 bytes leaked per early return before).
- Nested expressions in placeholders kept fragment-relative spans (`grown` in
  `{grown < 500}` at offsets 0..5), which aliased the type and call resolution
  of unrelated code at those offsets: that `grown` compiled as a reference to
  a function and the comparison printed `false`. Fragment tokens are now
  shifted to the literal's file position (offsets counted in Unicode scalar
  values), so nested spans stay inside the literal.

Second review follow-up: placeholder offsets count decoded characters, so
after an escape (`\u{1F600}`) they pointed into the escape and a rename
replaced part of it. Rename now refuses when the text at any use is not the
binding's name (references skip such uses), and placeholders on later lines
of a multi-line literal report their real line and column.

Verification: `scripts/check_string_interpolation.py` runs 400000 calls whose
placeholder `?` leaves on every other call (mapped memory stays flat), a unit
test pins nested placeholder spans, and `scripts/check_thread_atomics.py` gained a 4-thread
format-and-parse-back program (0 mismatches over 3 runs; the previous
compiler reports about 310000 per run), full `cargo test`, every
`scripts/check_*.py`, std `qz test` and all examples.
