# `std.tls`

Audience: Quazi application developers.

`std.tls` adds TLS client streams and HTTPS requests on top of
[`std.net`](net.md). It follows
[D-008](../decisions/README.md#d-008-tls-and-trust-policy): the system TLS
library and the system trust store, with certificate and host-name
verification always on. Linux and NetBSD use OpenSSL 3 (NetBSD 11: the base
system's `libssl.so.16`, linked by the built-in NetBSD linker). Windows uses
Schannel (TLS 1.2) and verifies the chain and host name through the
certificate chain API (system store, or only the CA file's PEM certificates).

## Linking

The module calls OpenSSL through `@api` declarations. Only programs that call
into it reference those symbols, and they link libc and the system libraries:

```toml
[target.x86_64-linux.link]
libc = true
libraries = ["ssl", "crypto"]
```

Add `library-paths` when the libraries live outside the linker's default
search path. At run time the dynamic loader must find `libssl.so.3`.
Windows needs no configuration: the program imports `secur32.dll` and
`crypt32.dll`, which every Windows installation has.

## Streams

`TlsStream.connect(host, port)` opens a TCP connection, sends SNI for a DNS
name (not for an IPv4 address), negotiates TLS 1.2 or newer and verifies the
server: the chain against the system store and the certificate against
`host`. `TlsStream.connect_with(host, port, &config)` takes a `TlsConfig`:
`TlsConfig.system()`, or `TlsConfig.with_ca_file(path)?` to trust only the PEM
certificates in `path` (a private CA; an empty path is
`TlsError.InvalidConfig`). No API disables verification.

`send(text)`, `send_bytes(data)`, `receive(limit)` and `receive_all(limit)`
take an exclusive receiver and mirror `TcpStream`. `receive` returns `""`
after the peer's close_notify. `receive_all` reads until close_notify; a
connection that ends without it is `ProtocolError`, so a truncated response
is never returned as complete. Writes block SIGPIPE for the calling thread
while OpenSSL writes, so a peer that closed the connection yields
`ProtocolError` instead of killing the process; a SIGPIPE already pending
stays pending. `close()` sends close_notify (skipped after a fatal error) and
releases the session and socket; it is repeat-safe and runs at scope exit.

## HTTPS

`tls.send_url(request, url)` and `tls.send_url_with(request, url, &config)`
send an `HttpRequest` to an `https` `Url` the way `HttpRequest.send_url` does
for `http`: the URL supplies host, port and request target; method, headers,
body and response limit are kept, and `Host` gains the port when it is not
443. `HttpRequest.send_url` itself still rejects `https` with
`TlsUnavailable`, so plain HTTP code never downgrades a secure URL.

## Errors

`TlsError` covers `Net(NetError)` for TCP and DNS failures, `InvalidName(index)`
for a NUL in the host or CA path, `TrustStoreUnavailable`,
`UntrustedCertificate(code)` with the X.509 verification code,
`HostnameMismatch`, `HandshakeFailed(code)` and `ProtocolError(code)` with the
`SSL_get_error` code, `Closed`, `LimitExceeded`, `InvalidUtf8`,
`AllocationFailed`, `LibraryFailure` and `Unsupported`. `message()` returns
display text.

## Verification

`python3 std/scripts/check_tls.py --qz <qz>` (workspace root) builds a client
and runs it against a local Python TLS server with a throwaway CA: trusted CA,
untrusted system store, wrong host name, missing CA file, truncation, a peer
that closes while the client writes, and a refused connection. Pass `--openssl` and `--ssl-lib` when the tools are not on
the default paths.
