# `std.net`

Audience: Quazi application developers.

`std.net` currently provides IPv4 TCP and UDP sockets plus small HTTP/1.1
request, response, and one-request server helpers on Linux and Windows. It is
not a general asynchronous networking framework: sockets are blocking, there
are no configured timeouts, IPv6, proxy support, TLS, or connection pooling.

## Errors and text boundary

Fallible APIs return `NetError`. Common conditions have stable variants
(`ConnectionRefused`, `AddressInUse`, `TimedOut`, `LimitExceeded`,
`InvalidUtf8`); unexpected platform errors remain `Native(i32)`. Use
`message()` for display rather than matching its text.

Text receive APIs validate UTF-8 before constructing `String`. Invalid bytes
return `InvalidUtf8`; binary protocols must use the unsafe/raw boundaries not
yet exposed as a complete high-level binary-stream API. Receive limits are byte
limits. A `TcpStream.receive` may read additional bytes only to complete a
trailing UTF-8 scalar, and returns `LimitExceeded` if that scalar would exceed
the requested limit. `receive_all` requires EOF before the limit and returns
`LimitExceeded` when the buffer fills.

## Addresses

`SocketAddress.new(host, port)` validates a non-empty host and port `0..65535`.
`host()` and `port()` expose its fields. `resolve()` performs a platform DNS
lookup and returns an IPv4 numeric address; it does not provide a multi-address
result, IPv6, DNS timeout, caching, or a configurable resolver policy.

## TCP

`TcpStream.connect(host, port)` creates a connected blocking IPv4 stream.
`send(text)` and `send_bytes(bytes)` retry until the full input is sent or a
network failure occurs, returning the complete byte count. `send_raw(ptr, len)`
is unsafe because the caller must provide readable storage. On Linux, sends use
the per-call no-SIGPIPE flag, so a closed peer reports `NetError.BrokenPipe`
instead of terminating the process.

`receive(limit)` returns one available UTF-8 text read; `receive_all(limit)`
reads until peer EOF. `recv(ptr, len)` is unsafe and returns the native count
or failure sentinel. `shutdown(Shutdown.Read|Write|Both)` requests a half- or
full shutdown. `close()` invalidates the socket and `free()` is its automatic
destructor; `handle()` is target-specific interop only.

`TcpListener.bind(port)` listens with backlog 128;
`bind_with_backlog(port, backlog)` selects the backlog. `accept()` returns an
owning `TcpStream`. Listeners have the same `close`, `free`, and `handle`
semantics as streams. These owners are not a shared, thread-safe socket API.

## UDP

`UdpSocket.bind(port)` creates a local IPv4 datagram endpoint;
`UdpSocket.connect(host, port)` selects a remote peer. `send(text)` sends to
that peer and may return a short count; `send_to(text, address)` sends one
datagram to an explicit resolved IPv4 address. No binary UDP send method is
currently public.

`receive(limit)` returns one UTF-8 datagram payload. `receive_from(limit)` also
returns a `UdpDatagram`, whose `text()` borrows the payload and `address()`
returns the source address. A zero `receive_from` limit returns
`MessageTooLarge`. A too-small nonzero buffer follows platform datagram
semantics; callers should choose a limit large enough for the protocol’s maximum
payload. `UdpSocket` provides `close`, automatic `free`, and `handle` like TCP.

## HTTP/1.1 helpers

`HttpMethod` supplies `Get`, `Head`, `Post`, `Put`, `Patch`, `Delete`,
`Options`, `Connect`, and `Trace`; `name()` returns the wire spelling.
`Url.parse` accepts only `http` and `https`, host, optional numeric port, and a
path (default `/`). It is intentionally a small parser, not a general URL
implementation: no IPv6 literals, authority userinfo, query normalization, or
percent-decoding contract is supplied.

`Headers.new`, `append`, `set`, `get`, `contains`, `len`, `get_at`, and `encode`
manage ordered headers. Lookup and replacement are case-insensitive; encoding
preserves inserted spelling and order. Header names and values are not validated
against HTTP control-character rules, so callers must not pass untrusted text
without their own validation.

`HttpRequest.new(method, target, host)`, `with_method`, and `from_url` build a
request. `header` replaces a header case-insensitively, `body` changes its
owned text body, and `max_response_bytes` sets the `receive_all` cap used by
`send`. `encode` adds absent `Host`, `Content-Length`, and `Connection: close`
headers. `send(port)` connects, sends, reads one complete response, closes the
stream, and parses it. `send_url(url)` rejects HTTPS with `TlsUnavailable`; it
does not silently downgrade transport security. `parse(raw)` parses an owned
request text.

`HttpResponse.new`, `text`, `header`, `status`, `reason`, `headers`, `body`,
`encode`, and `parse` provide the matching response surface. `text` supplies a
small conventional reason phrase table and a UTF-8 text content type. Parsing
recognizes HTTP/1.0 and HTTP/1.1, validates basic framing, and decodes chunked
bodies with a fixed 16 MiB limit. It is not a hardened general HTTP parser:
there is no header-count/line-size policy, trailer API, compression, redirect,
cookie, or authentication support.

`HttpServer.bind(port)` wraps a `TcpListener`; `accept()` returns one owning
`HttpConnection`. A connection can `read(limit)` into `HttpRequest`,
`read_request(limit)` into raw text, `respond(response)` (which closes after
the send), or `close()`. The server itself provides `close`, automatic `free`,
and `serve_once(response, limit)`, which accepts one client, returns its raw
request text after responding, and closes both connection and listener.
`http_request`, `http_get`, and `http_post` are compatibility helpers that
return only an owned response body; new code should use `HttpRequest` and
inspect `HttpResponse`. This is a one-request building block, not a concurrent
HTTP server runtime.

## Security and portability

HTTPS deliberately fails with `TlsUnavailable`; no certificate or hostname
verification exists in this module. Do not send credentials over HTTP. Linux
uses native sockets and Windows uses Winsock; error normalization is best-effort,
so portable applications must handle `Native` and `Unsupported`. Blocking
operations may wait indefinitely without external OS-level controls.
