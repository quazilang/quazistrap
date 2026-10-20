# Migrating loop cleanup and initializer moves

Audience: Quazi users rebuilding programs and dependencies.

Supported initialized owners are now destroyed before `break` and `continue`
leave their body scopes. A C-style initializer owner survives `continue` and
is destroyed when the loop exits, before the following statement. Previously
its cleanup could be delayed until the surrounding block ended. Move an owner
declaration outside the loop when its lifetime must extend beyond the loop.

Initialization executes once. Moving an outer owner into a C-style initializer
is allowed, and the outer binding remains moved afterward. Consuming the
initializer owner from a repeated condition, body or update is rejected with
`S10`, matching the existing rule for outer owners used inside loops. Borrow
for repeated inspection, or create an owner inside the body for each iteration.
Initializer reference loans end with the loop, so they no longer prevent a
later legal move of the outer owner.

QZC20 invalidates older source caches automatically. Rebuild old QZI libraries
from source to obtain the cleanup and optimizer corrections. Emitting native
code from an old body does not add its missing cleanup calls or undo incorrect
constant folding. The QZI format and calling convention remain unchanged;
QZI13 ownership envelopes remain Unverified.

See the [change record and checks](../changes/2026-10-20-loop-cleanup.md) and
the [scope-cleanup contract](../language/safety.md#scope-cleanup-raii).
