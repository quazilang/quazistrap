# Native Object Metadata Omission

Audience: Quazi users and compiler maintainers.

## Change

The built-in ELF linker now omits GNU property notes and `.eh_frame` unwind
metadata from native C objects. These sections do not participate in Quazi's
freestanding executable image. Unsupported allocated sections, including TLS,
remain errors.

## Compatibility and Migration

This is a bug fix with no source migration. Ordinary C objects containing this
metadata now link with the built-in linker. The linker still does not provide a
dynamic loader or unwinder.

## Verification

- A linker regression covers GNU property-note omission while preserving TLS
  rejection.
- `examples/19-c-interop` builds with the local compiler.
