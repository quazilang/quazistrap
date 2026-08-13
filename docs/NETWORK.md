# `std.net`

`std.net` provides cross-platform TCP plus small HTTP/1.1 client and local
server building blocks. Linux uses sockets; Windows uses Winsock.

- TCP connect, bind, listen, accept, complete send, bounded receive, close.
- HTTP request creation and response status/header/body parsing.
- One-request local server accept/read/respond flow.
- No HTTPS claim: TLS requires a separate implementation or dependency.

See `examples/26-http-client-server`. Close network resources on success and error paths;
keep receive limits explicit.
