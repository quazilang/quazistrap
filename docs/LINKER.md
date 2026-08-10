# Built-in Linux Linker and Runtime

Quazi's experimental linker produces a static x86-64 Linux ELF executable
without invoking `ld`, `lld`, `mold`, GCC, Clang, libc, or CRT startup code.
Linking is part of `qz build` and `qz run`; there is intentionally no separate
link command.

## Selecting a linking path

The built-in linker is selected when all of the following are true:

- the target is x86-64 Linux;
- neither `--linker <external-path>` nor an external `QUAZI_LINKER` is set;
- every additional native input is an ELF `.o` file; and
- no library, archive, shared object, library path, or other native linker flag
  was requested.

`--linker builtin` or `QUAZI_LINKER=builtin` requires this path. If an input is
unsupported, Quazi returns an error rather than falling back to a host linker.
The command-line `--linker` value takes precedence over `QUAZI_LINKER`.

The external path is selected by any explicit external linker, `-l`, `-L`,
archive/shared-library input, shared-library output, or unsupported target.
External Linux linking still adds neither libc nor libm automatically. Windows
adds the OS import libraries needed by its generated startup code
(`kernel32.lib` and `shell32.lib`), but does not add the Universal CRT,
`libcmt`, `vcruntime`, or legacy stdio libraries.

## Supported input workflows

The input list may combine Quazi source with objects:

```bash
qz build src/main.qz native/codec.o native/hash.o -o app
qz run src/main.qz native/codec.o
```

Portable QZI may be combined with objects in the same way:

```bash
qz build program.qzi native/codec.o -o app
qz run program.qzi native/codec.o
```

Object-only builds are also accepted:

```bash
qz build program.o helper.o -o app
qz run program.o helper.o
```

Project `[link].objects` and objects produced from project `[cc].sources` enter
the same pipeline. During QZI project discovery, identical object paths
supplied both explicitly and by the project are deduplicated. A project can
declare native inputs explicitly:

```toml
[cc]
sources = ["native/helper.c"]
include-paths = ["native/include"]
defines = ["FEATURE=1"]
flags = ["-Wall"]

[link]
objects = ["native/prebuilt.o"]
libraries = ["sqlite3"]
library-paths = ["native/lib"]
```

`[cc]` intentionally invokes a C compiler to create its `.o` files. A project
which supplies prebuilt `[link].objects` and no libraries can remain on the
built-in linker path; `[link].libraries` or `[link].library-paths` select the
external path.

`qz build -c` remains compile-only and emits a relocatable object. Normal
source/QZI executable builds contain the compiler's full `_start`. If an
object-only build defines `main` but not `_start`, the linker adds a minimal
15-byte entry stub which calls `main`, passes its return value to `SYS_exit`,
and performs the syscall. This fallback does not construct `Array[str]`, scan
`QUAZI_TRACE`, or install Quazi's crash signal handlers; use a normal source or
QZI executable build when those facilities are required.

## ELF input contract

Every built-in input must be a little-endian x86-64 ELF relocatable object. The
linker currently lays out these section kinds:

- executable code;
- read-only data and strings;
- initialized data;
- uninitialized data; and
- ELF common symbols, with their requested size and alignment.

It resolves local symbols within each object and global symbols across all
objects. Multiple strong definitions are errors. A strong definition wins over
a weak definition; an unresolved weak symbol resolves to zero.

Supported relocations are deliberately small and checked:

| Relocation | Width | Use |
|------------|-------|-----|
| PC-relative / PLT-relative | 32 bits | Calls and RIP-relative references |
| Absolute | 64 bits | Absolute addresses in data/code |

Overflow, an unsupported relocation target/type, a malformed object, duplicate
strong symbol, or unresolved non-weak symbol stops the build with a diagnostic.
The linker does not guess, truncate, or silently import a library.

Metadata and non-loaded debugging sections do not appear in the final image.
Objects that depend on other allocated section types, TLS, COMDAT/group
selection, symbol versioning, linker scripts, dynamic relocations, or archive
member extraction are outside the current contract.

## Output contract

The result is an `ET_EXEC` ELF64 image based at `0x400000` with:

- one read/execute `PT_LOAD` segment containing headers and code;
- one read-only `PT_LOAD` segment containing constants;
- one read/write `PT_LOAD` segment containing mutable and zero-initialized data;
- a read/write, non-executable `PT_GNU_STACK`; and
- 4 KiB page alignment.

Relocation arithmetic, alignment, offsets, image sizes, and relative-call ranges
are checked before writing. The output has no dynamic interpreter, dynamic
symbol table, runtime loader dependency, or ELF section-header table.

## Embedded Linux runtime

The x86-64 backend scans unresolved calls after code generation and emits only
the assembly routines the object requests. Supported routines are:

| Area | Routines |
|------|----------|
| Allocation | `malloc`, `calloc`, `realloc`, `free` |
| Memory | `memcpy`, `memmove`, `memset`, `memcmp` |
| C strings | `strcpy`, `strcat` |

Allocation uses Linux `mmap` and `munmap` syscalls. Each allocation has a
private 16-byte header containing mapping and requested sizes. `calloc` checks
multiplication overflow and zeroes successful allocations; `realloc` copies the
smaller old/new requested length and releases the old mapping; `free(NULL)` is
a no-op.

Integer decimal, hexadecimal, octal, and binary conversion is generated inline.
Floating-point conversion is also generated inline, including sign,
`inf`/`nan`, fixed precision from zero through nine digits, trailing-zero
trimming for the default representation, and fractional rounding carry. These
paths do not import `sprintf`.

Linux `_start` reads `QUAZI_TRACE=1` directly from the kernel-provided initial
stack. The full generated startup and crash handler therefore do not need
`getenv` or a dynamic loader.

## Explicit native libraries

Native libraries are allowed, but always intentional. For example, a program
which imports C `printf` must request libc:

```bash
qz build src/main.qz -l c -o app
```

This selects the external linker and adds a dynamic interpreter only because a
dynamic library was requested. `--linker builtin -l c` is rejected because the
built-in linker does not implement dynamic linking.

An unresolved-symbol error from the built-in path is therefore useful: either
implement the dependency in Quazi/the embedded runtime, pass another ELF `.o`
which defines it, or explicitly opt into the native library and external
linker.

## Current limitations

- x86-64 Linux static executables only;
- no PE/COFF, Mach-O, or cross-target built-in linker;
- no `.a` archive extraction or `.so` dynamic linking;
- no TLS, COMDAT/group selection, symbol versions, linker scripts, or section
  garbage collection;
- only the relocation forms listed above;
- object-only synthesized startup supports a no-argument `main` contract only;
- the embedded float formatter is a lightweight fixed-format implementation,
  not a complete locale-aware `printf`/shortest-roundtrip replacement.

Use the experimental linker for Quazi-produced objects and small, compatible
native objects. Select an external linker explicitly when a dependency needs a
general-purpose system linker feature.
