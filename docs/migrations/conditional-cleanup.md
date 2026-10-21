# Migrating conditional owner cleanup

Audience: Quazi users rebuilding programs and dependencies.

Cleanup now follows runtime initialization and movement. An owned local whose
initializer branch is skipped is not destroyed. An initialized owner survives
an untaken consuming branch and is destroyed at its normal scope exit. A
`match` result transfers the selected owner while other owners retain their
cleanup. Replacement destroys the previous value only if it is still owned.

QZC21 invalidates older source caches automatically. Rebuild older QZI libraries
from source to obtain runtime cleanup flags; emitting an old QZI body again
does not repair its cleanup. The QZI format and calling convention are unchanged,
and QZI13 ownership envelopes remain Unverified.

These cleanup corrections do not establish definite initialization for ordinary
reads. Initialize values on every path before reading them. Existing restrictions
on repeated loop consumption and partial owned-field moves still apply.

See the [change record and checks](../changes/2026-10-21-conditional-cleanup.md)
and [scope cleanup](../language/safety.md#scope-cleanup-raii).
