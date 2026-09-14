# Duration receiver capabilities

`std.time.Duration` accessors and checked arithmetic now use shared receivers.

## Why

Reading duration components and computing another normalized duration do not
mutate the receiver. Expressing that contract explicitly lets a caller retain
the value for later use and fits the staged migration before bare `self: T`
becomes consuming under D-014.

## Compatibility

Existing calls remain source-compatible. `checked_add` and `checked_sub`
continue to accept their right-hand operand by value; only the receiver's
capability changed.

## Verification

The `std.time` source-level duration and monotonic-instant tests run through
the contained compiler, and the canonical documentation suite checks the API
reference.
