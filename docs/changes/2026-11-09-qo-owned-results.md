# `qo` returns owned results

Audience: language users and compiler developers.

The `qo` closure's result had to be a plain scalar (`S10 closure return type
... until closure result ownership is implemented`), so `qo label(n)`
returning a `String` or a struct was rejected.

A `once` closure (the `qo` lowering) may now return a named owned type. The
value is produced once, moved into the task's result slot by the prelude
runner, and moved out by `join()`. `join` now reads the slot only when the
qoroutine finished without panicking, so a panic never makes it adopt an
unwritten slot as an owned value. Ordinary closures keep the scalar rule.

Verification: `scripts/check_qoroutines.py` returns `String` and struct
(with a `String` field) results from 400 qoroutines, and owned results next
to 20 panicking qoroutines; a semantic regression covers the closure rule.
