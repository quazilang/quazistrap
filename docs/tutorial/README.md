# Quazi tutorial

Audience: new Quazi users.

This progressive tutorial takes you from your first program through the
complete language. Each chapter builds on the previous one and links to
runnable examples from the [`examples/`](../../examples/README.md) directory.

## Chapters

1. [Hello, World!](01-hello-world.md) — installation, first program, project
   structure.
2. [Variables and types](02-basics.md) — constants, variables, numeric types,
   strings, arithmetic.
3. [Control flow](03-control-flow.md) — conditionals, loop forms,
   break/continue, pattern matching.
4. [Functions](04-functions.md) — parameters, generics, closures, ownership.
5. [Structs, enums, and traits](05-types.md) — custom types, methods,
   interfaces, dynamic dispatch.
6. [Modules and packages](06-modules.md) — imports, visibility, dependencies,
   libraries.
7. [Error handling](07-error-handling.md) — `Result`, `Option`, the `?`
   operator, panics.
8. [Collections](08-collections.md) — arrays, maps, sets, Box.
9. [File and console I/O](09-io.md) — reading, writing, filesystem, console
   input.
10. [Building a complete program](10-practical.md) — a realistic end-to-end
    application combining all concepts.

## Prerequisites

- A Quazi compiler (`qz`) built from `quazistrap`.
- A text editor. See [tooling](../tooling/README.md) for editor support.

## Style

All examples use current Quazi syntax and are designed to compile with the
current compiler. Features labeled experimental in the
[language specification](../language/README.md) are noted where they appear.
