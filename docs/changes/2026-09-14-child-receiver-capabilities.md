# Child receiver capabilities

`std.process.Child.terminate` now requires an exclusive receiver, while
`handle` uses a shared receiver.

## Why

Termination changes external process state and must not overlap a safe shared
loan of the child owner. Reading the native handle only inspects the owner.
These capabilities make that distinction enforceable while the staged D-014
migration retains legacy by-value receivers for genuinely consuming methods
such as `wait`, `try_wait`, and `close`.

## Compatibility

Calls on mutable local children continue to work. Code holding `&Child` can
call `handle`, but must end that loan before `terminate`.

## Verification

The standard-library process source tests compile and run with the contained
compiler, and the canonical documentation suite validates the public contract.
