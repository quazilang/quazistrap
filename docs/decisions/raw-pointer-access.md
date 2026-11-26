# D-027: read-only `*T` and writable `*T!` raw pointers

Decided 2026-11-26 by the maintainer: raw pointers distinguish access like
references do.

| Type | Meaning |
|---|---|
| `&T` | shared safe reference |
| `&T!` | exclusive safe reference |
| `*T` | raw pointer for reading |
| `*T!` | raw pointer for reading and writing |

Rules (checked by the typechecker; dereferencing stays `unsafe`):

- A write through a pointer (`*p = v`, `*p += v`, `(*p)++`,
  `(*p).field = v`, `(*p)[i] = v`) needs `*T!`; through `*T` it is S07.
- `*T!` converts implicitly to `*T` (and to `*U`, pointee types stay C
  `void*`-compatible); `*T` becomes `*T!` only with an explicit `as *T!`.
- `&x` passes as `*T`, `&x!` as `*T` or `*T!`.
- Only the literal `0` converts to a pointer (the null constant). An integer
  value no longer converts implicitly, and pointer/integer casts stay
  rejected so pointer provenance remains analyzable (retention analysis).
- Pointers compare with pointers and integers (`p == 0`); `p + n` / `p - n`
  offset by bytes and keep `p`'s type. The difference of two pointers is not
  supported (it would need a pointer-to-integer conversion).
- A raw pointer has no fields or elements: `(*p).field` and `(*p)[i]`, never
  `p.field` / `p[i]` (S11; those forms were never lowered through the
  pointer).
- Inside a reference or a generic argument access is invariant: `&*u8` is
  not `&*u8!`, `W[*u8]` is not `W[*u8!]`.
- `if`/`match` arms of `*T!` and `*T` give `*T`, in either order.
- `!` applies to a pointer written in place, not to an alias: with
  `type P = *u8;`, `P!` is not a type.
- `**T` is `*(*T)`; `**T!` makes the outer pointer writable; a writable inner
  pointer is spelled `* *T!` (types print that way and reparse identically;
  the two forms without a source spelling print as `&(*T)!` and `*(&T)!`).
- Allocators return `*u8!` (`core.malloc`, `core.realloc`); copy/fill
  destinations take `*u8!` (`core.memcpy`, `memset`, `memmove`).

`@api` declarations describe the C contract: C writes are not checked by
Quazi, so out-parameters are documented as `*T!` where std was migrated.

Compatibility: no QZI format change; types are stored as text, and an old
`*T` signature now reads as read-only (callers may still pass `*T!`).
Migration: [raw pointer access](../migrations/raw-pointer-access.md).
