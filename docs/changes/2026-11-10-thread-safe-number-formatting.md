# Numbers in format arguments get private digit buffers

Audience: language users and compiler developers.

`PrimToStr` (number or bool to text) wrote its digits into one static
`__quazi_itoa_*` buffer per instruction site. Two threads (or qoroutines on
different workers) running the same `println`, `format` or interpolated
literal overwrote each other's digits: 4 threads formatting 100000 numbers
each and parsing them back saw about 310000 mismatches per run.

Inside a format call (`fmt.format`, `println`/`print` wrappers, interpolated
literals, `@format` instance methods and other `str...` variadic calls) codegen
now allocates an 80-byte heap buffer per numeric argument, passes it to
`PrimToStr` in `ops[3]` with `PRIM_TO_STR_BUFFER_FLAG`, and frees it after the
call returns; the call's result is an owned `String` that no longer refers to
the buffer. Register allocation and remapping treat `ops[3]` as a use when the
flag is set. Outside format calls `PrimToStr` keeps the static buffer, so a
direct `n.to_str()` view is still per-site shared state: the API decision is
open as question В9 in the workspace `asks.txt`, and the format path is safe
under either answer.

Verification: `scripts/check_thread_atomics.py` gained a 4-thread
format-and-parse-back program (0 mismatches over 3 runs; the previous
compiler reports about 310000 per run), full `cargo test`, every
`scripts/check_*.py`, std `qz test` and all examples.
