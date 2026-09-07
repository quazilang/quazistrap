# URL-targeted HTTP requests and numeric IPv4 routing

Audience: Quazi network users and maintainers.

`HttpRequest.send_url(url)` now uses the URL host, port, and path for the
connection and request target. It preserves the existing method, body, headers,
and response limit. If no `Host` header was supplied, it emits the URL authority
including a non-default HTTP port; an explicit `Host` remains an intentional
virtual-host override. HTTPS still returns `TlsUnavailable` before connecting.

Valid dotted-decimal IPv4 addresses now bypass DNS lookup in `SocketAddress`
resolution and are parsed by the same strict parser used to construct the
socket address. Host names retain the existing platform DNS behavior.

The `26-http-client-server` example includes `http-url-client`, which builds a
request with a deliberately incorrect host and target, sends it to a loopback
URL, and verifies the HTTP response. The server's captured request must show
the URL path and `Host` authority.
