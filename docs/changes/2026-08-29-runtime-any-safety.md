# Runtime `any` safety boundary

## Motivation

`any` previously acted as a universal compatibility wildcard even though the
VM, native backend, and QZI format had no dynamic tag or payload layout. Values
could therefore be silently reinterpreted as unrelated register shapes,
including pointers and strings.

## Behavior

Source `any` is now rejected in every value-bearing type position. Use a
generic parameter, concrete type, or `dyn Trait` instead. Internal recovery and
incomplete inference use a compiler-only `Error` sentinel that cannot reach
code generation. Closures need a contextual `fn(...) Return` signature, and
dynamic trait calls retain the method's concrete declared signature.

The supported exception is a final `...args: any` parameter on an `@format`
function. This is a pseudo-parameter: arguments are converted at the call site,
and no runtime `any` value enters the callee. Public QZI interfaces reject all
other `any` uses. Public generic functions, types, traits, and methods also
require source distribution because QZI does not carry generic template bodies.
Impl-only interface modules are loaded for their semantic side effects instead
of being dropped from the materialized dependency gateway.

`std.thread` now accepts exact target-specific `@repr(C)` callback aliases:
`fn(*u8) *u8` on Linux and `fn(*u8) u32` on Windows. Thread callbacks must be
compatible `@export` functions. Linux thread creation now returns zero when
temporary handle allocation or `pthread_create` fails and frees the temporary
allocation on creation failure. Windows preserves `CreateThread`'s null-handle
failure result.

Joining a zero handle is a safe no-op on both targets, preventing the current
high-level wrapper from turning a reported spawn failure into a null dereference.

## Compatibility

Programs that relied on implicit `any` conversions no longer compile. This is
a safety correction. Existing format-style functions remain source-compatible
when they carry `@format`. Libraries with public runtime `any` APIs must be
published as source only long enough to migrate their signatures; rebuilding a
QZI does not make an unrepresentable API safe.

QZI v6 trait interfaces with parameters must be rebuilt. That writer replaced
source parameter names, including `self`, with synthetic `argN` names. The v7
analyzer uses the preserved name to distinguish explicit receivers, so guessing
would change method arity and dynamic-dispatch shape. The reader now fails with
an actionable rebuild-or-source message instead of silently misinterpreting the
interface.

QZI v6 public interfaces containing runtime `any` are also rejected while the
artifact is decoded, before a direct QZI dependency can bypass source semantic
checks. Rebuild after migrating the API, or distribute the dependency as source.

## Verification

Compiler regressions cover rejected runtime positions, strict compatibility,
format erasure, contextual sum constructors, unresolved inference, closure
context across every callable route, safe trait implementation contracts,
dynamic trait signatures, and QZI interfaces. Standard-library thread
callbacks compile to Linux and Windows native objects with exact C ABI types.
Backend instruction and relocation coverage verifies both Linux failure checks,
failure cleanup, and the target-specific Windows path.

The latest combined safety checkpoint ran `cargo test --offline` (410 tests), the testing
example suite (6 tests), and Linux ELF plus Windows COFF thread object builds.
