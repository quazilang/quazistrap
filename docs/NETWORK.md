# `std.net`

`std.net` provides cross-platform IPv4 TCP and UDP plus small HTTP/1.1
client and local server building blocks. Linux and NetBSD use sockets (Linux
builds need `[target.x86_64-linux.link] libc = true` for name resolution);
Windows uses Winsock.

- TCP connect, bind, listen, accept, complete send, bounded receive, close.
- UDP bind, connect, `send_to`/`receive_from`.
- HTTP request creation and response status/header/body parsing.
- One-request local server accept/read/respond flow.
- HTTPS goes through [`std.tls`](api/tls.md) (Linux, system OpenSSL 3).

See `examples/26-http-client-server`. Close network resources on success and error paths;
keep receive limits explicit.

All safe network operations return `Result[T, NetError]`. Stable variants such
as `ConnectionRefused`, `AddressInUse`, `TimedOut`, and `NetworkUnreachable`
hide platform differences. Call `error.message()` for readable output.
`Native(code)` retains unexpected OS errors without making raw codes the normal
public contract.
