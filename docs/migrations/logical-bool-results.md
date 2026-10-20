# Logical integer operands produce boolean results

Audience: language users maintaining existing applications and QZI libraries.

`&&` and `||` accept booleans and integers, interpret zero as false and nonzero
as true, and return `bool`. Older generated code could return the raw integer
RHS when that operand was evaluated. This made a truthy Bool value compare
unequal to `true`. The compiler now normalizes that path to 0/1, matching the
already-canonical shortcut path. RHS evaluation remains lazy.

Code using logical expressions as numeric selectors should use explicit
`if`/`else` control flow to choose a numeric value. Boolean conditions and
comparisons should use the corrected logical result directly.

Source projects rebuild automatically because QZC18 rejects previous exact and
reusable caches. Rebuild affected QZI libraries from their source as well:
emitting an existing `.qzi` again does not rewrite its logical instructions.
The QZI format remains v13; old artifact bodies retain their emitted behavior.

See [the change and verification record](../changes/2026-10-20-expression-control.md).
