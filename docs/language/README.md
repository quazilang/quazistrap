# Language specification

Audience: language users and implementers.

This is the authoritative specification for the Quazi programming language.
It describes every supported language feature based on the current compiler
implementation. Features labeled experimental, incomplete, or
implementation-defined are clearly marked.

## Contents

- [Lexical structure](lexical.md) — source encoding, comments, identifiers,
  keywords, literals, escape sequences, tokens.
- [Type system](types.md) — primitives, integers, floats, text, bytes,
  containers, references, pointers, named types, generics, function types.
- [Expressions](expressions.md) — operators, precedence, calls, indexing,
  closures, match, ranges, error propagation.
- [Statements and control flow](statements.md) — bindings, assignment,
  return, conditionals, loop forms, break/continue, unsafe blocks.
- [Declarations](declarations.md) — functions, structs, enums, traits, impl
  blocks, type aliases, derive, visibility.
- [Modules and imports](modules.md) — import syntax, visibility, package
  structure, dependencies, deduplication.
- [Safety and ownership](safety.md) — unsafe system, ownership model, scope
  cleanup, references, raw pointers, function value ownership.
- [Attributes](attributes.md) — `@cfg`, `@api`, `@export`, `@repr(C)`,
  `@opaque`, `@syscall`, `@intrinsic`, `@derive`, `@inline`, `@ignore`,
  `@test`, `@panic_handler`, field attributes.
- [Panic handling](panic.md) — process termination and the custom-handler
  ABI.
- [Platform and targets](platform.md) — conditional compilation, supported
  targets, platform-specific behavior, implementation-defined behavior.

The existing [language guide](../LANGUAGE.md) and
[types and memory guide](../TYPES_AND_MEMORY.md) contain earlier material
that has been incorporated into the pages above.
