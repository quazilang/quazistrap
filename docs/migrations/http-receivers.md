# Migrating HTTP receiver code

Audience: applications using `std.net`.

Status: receiver signatures are migrated; full HTTP integration still requires
[D-015 element loans](../decisions/borrowed-elements.md) and
[D-014 ownership effects](../decisions/whole-program-ownership.md).

Keep mutable connection and server bindings when performing socket operations:

```quazi
var server = HttpServer.bind(8080).unwrap();
var connection = server.accept().unwrap();
const request = connection.read(65536).unwrap();
connection.respond(HttpResponse.text(200, "ok")).unwrap();
connection.close();
server.close();
```

This demonstrates the intended receiver usage; it is not currently a runnable
HTTP example while the header/codec integration remains unbuildable.
`read`/`read_request` borrow the connection exclusively so it remains available
for a later response or close. `accept` borrows the server exclusively so it
remains available for another accept or close. Both close operations are
idempotent. `serve_once` still consumes the server; server `free` remains a
consuming destructor.

Datagram `text`, request `method`/`target`/`host`/`body_text`, and response
`status`/`reason`/`body` are shared observations. Keep an owning local alive for
returned text. A bound view prevents moves and mutation of that owner; copy it
into `String` when independent ownership is needed. A temporary owner cannot
supply a retained view. Datagram `address` remains a consuming transfer.

Owned dense elements cannot be copied with either shared methods or `[]`,
including through a concrete generic helper. Replace unsupported reads only
with approved ownership operations; do not cast raw storage into a second owner
or a safe reference. Indexed replacement and exclusive removal retain their
existing contracts. The borrowed-element API is still pending.

Rebuild source to receive the new diagnostics. QZC v10 discards prior caches
automatically so exact hits cannot preserve the old validation policy. This
change does not certify or repair existing QZI artifacts. See the
[change record](../changes/2026-10-13-http-receiver-ownership.md) for verification
scope and remaining failures.
