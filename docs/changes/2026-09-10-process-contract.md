# Process contract approval

Audience: Quazi users and runtime maintainers.

D-011 now has an approved minimal public child-process contract: shell-free
exact paths, arguments excluding `argv[0]`, inherited standard streams,
numeric/signal exit status, forced termination, and close that reaps a live
child. Pipes, custom environments, working directories, timeouts, cancellation,
and PATH lookup remain deferred.

This records a design decision only; compiler runtime primitives and
`std.process` are not implemented by this change.
