# Network value receiver capabilities

Read-only `SocketAddress`, `Header`, and `Url` methods in `std.net` now use
shared receivers.

## Why

Component access and address resolution inspect an owner without changing it;
the returned text is borrowed, scalars are copied, and resolution creates a
fresh address. Shared receivers make those facts explicit while retaining
exclusive/consuming migration work for socket and HTTP lifecycle operations.

## Compatibility

Existing local-owner calls remain source-compatible. A shared view can now use
these methods without allowing mutation of the original value.

## Verification

The standard-library network source tests run through the contained compiler,
and the canonical documentation suite validates the public API reference.
