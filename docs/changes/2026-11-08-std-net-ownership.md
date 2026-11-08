# Usable `std.net` HTTP and UDP under ownership checking

Audience: language users and standard-library maintainers.

Every program that imported `std.net` failed analysis with about thirty `S10`
ownership errors, so examples `26-http-client-server` and `31-udp-echo` did not
build. The HTTP code read owned `Header` elements out of an `Array` by value,
used consuming receivers on fields (`self.headers.set`), moved fields out of
borrowed requests and responses, and `UdpDatagram.address()` consumed the
datagram whose text was still borrowed. `std.json.object` read owned `String`
elements the same way. These reads are deliberately rejected until
[borrowed dense-container elements](../decisions/borrowed-elements.md) exist.

## Changes

Standard library (`std/src/net.qz`, `std/src/json.qz`):

- `Headers` stores fields in one owned wire-format buffer. `append`/`set` take
  `&Headers!`; `get`, `contains`, `len`, `get_at`, `copy` and `encode` take
  `&Headers`. `get` now returns `Option[String]` (was `Option[str]`) and
  `get_at` returns an owned `Header` copy.
- `HttpRequest.headers`/`HttpResponse.headers` and both `encode` methods use
  shared receivers and copy the headers instead of moving fields.
- `HttpResponse.parse` is split into status-line and body helpers so it stays
  within the 255-register QZI function limit.
- `UdpDatagram.address()` takes `&UdpDatagram` and returns a copy.
- `json.object`/`object_with_limits` take each value with `Array.remove(0)`.

Compiler fixes found while repairing the module:

- Result retention treated `quazi.str.byte_at`, `quazi.strlen` and
  `quazi.memcmp` as opaque calls that could store their arguments anywhere, so
  any function reading a byte of a borrowed string appeared to retain it. These
  read-only intrinsics now publish no writes; their results still follow the
  declared result type.
- The move checker merged moves from every `if` branch into the following code,
  even from branches ending in `ret` or a `!` call such as `panic`. Such
  branches no longer affect later code; `break`/`continue` branches still do.
  `else if` conditions now also see moves made by earlier conditions.
- A private module function could not call a public function of the same
  module (`S04: '<name>' is not in scope`). Calls that resolve to the current
  module are now always allowed; entry code still needs an import.

`examples/26-http-client-server/src/url_client.qz` was split into check
functions because its `main` exceeded the 255-register QZI limit.

## Compatibility

Callers of `Headers.get` must accept `Option[String]`. Code that moved
`UdpDatagram.address()` out of a datagram keeps working. Callers of
`json.object` already passed `values` by value; it is now consumed. Source
caches older than QZC32 are rebuilt.

## Verification

- `cargo test --offline -q`: 995 passed, including new regressions for
  read-only intrinsic retention, diverging-branch moves (with falling-through
  and `break` counter-cases) and module-local sibling calls.
- All binaries of examples 26, 30 and 31 build. A local run of `http-server`
  with `http-client` and `http-url-client`, and of `udp-server` with
  `udp-echo`, completes successfully.
- `std` `@test` suite (excluding the isolated `os_memory` test): 32 passed.
  `scripts/check_os_memory.py` (6) and `scripts/check_net_receivers.py`
  (10 tests, negative lifetime probes, QZI/COFF) pass.
