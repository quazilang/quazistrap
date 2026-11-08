# Statement-scoped register reuse and the `Jz r0` fix

Audience: language users and compiler developers.

## Register limit

Code generation numbered every temporary with a fresh QZI register, so a long
function failed with `needs more than 255 QZI register slots` even when few
values were live (a test with 73 calls, `HttpResponse.parse`,
`json.decode_string`). After each statement the compiler now hands the numbers
of that statement's dead temporaries back, keeping every register still owned
by a binding, cleanup slot, closure environment or pending temporary. The
frame size and the inliner use the high-water mark. The 255-register limit
still applies to values live at the same time.

## Latent miscompilation: conditions in `r0`

Register allocation and register remapping treated `Jz r0` / `Jnz r0` as a
jump without a condition register, although the backend always tests
`slot(ops[0])`. A condition held in `r0` could have its definition deleted as
dead, its slot reused, or (after inlining) test the caller's `r0`. Register
reuse made this reachable (`var first: bool; ... ret first && second;`
returned false). Both now always treat `ops[0]` as the condition register.
QZC34 invalidates cached chunks produced with the old rule.

## Verification

Regalloc and remap regressions fail with the old rule; all compiler e2e
scripts, every example binary and the `std` suite pass. The 73-call program
compiles as one function.
