# Socket receiver capabilities

Audience: users of `std.net` and runtime maintainers.

TCP streams, TCP listeners, and UDP sockets now express their real ownership
requirements in method receivers. I/O, shutdown, accept, and close operations
take exclusive receivers because they operate on a mutable, single-owner OS
handle. `handle()` takes a shared receiver. `free(self)` remains the consuming
destructor used by lexical scope cleanup.

## Compatibility and migration

Socket variables that call an operational method must be declared `var`.
After `close()`, the owner remains valid in the explicitly closed state and
can be inspected or closed again; a final `free()` consumes it. Code that used
the owner after `free()` is rejected by the ownership checker and must move any
last inspection before that consuming call.

## Verification

The UDP lifecycle regression binds a real local socket, verifies close
invalidation and idempotence through the shared handle accessor, and then
performs final consuming cleanup. The compiler receiver-capability suite covers
the exclusive/shared/consuming enforcement underlying this API.
