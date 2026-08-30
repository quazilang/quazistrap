# AGENTS.md — Quazilang Project Reference

The primary reference for the Quazilang compiler (`qz`) — covers architecture, coding rules, language syntax, standard library, roadmap, and build pipeline.

---

## Quick Commands

```bash
cargo build              # debug build
cargo build --release    # release build
cargo test               # run all 152+ unit tests
cargo clippy             # lint
cargo fmt                # format
```

CLI (dep: `clap 4.6`):
```bash
qz build [source.qz|program.qzi|native.o ...] [-i|-c] [-o out] [-r] [-s] [--linker builtin|path] [--silent|--no-progress] [--no-color] [--no-unicode]
qz run [source.qz|program.qzi|native.o ...] [--linker builtin|path] [--silent|--no-progress] [--no-color] [--no-unicode] / qz check / qz fetch / qz deps / qz fmt / qz clean
qz test [filter] [--no-color] [--no-unicode]
qz header [file ...] [-o quazi.h] [--target x86_64-linux|x86_64-windows]
qz new <name> [--lib] / qz init [--lib]
qz add <path-or-url> [--alias name] [--type git|archive|source|qzi] [--version tag|hash|latest] / qz remove <name>
qz debug [-i]
qz lsp
```

Output: `<stem>.qzi` (bytecode), `<stem>.o` (object), `<stem>`/`<stem>.exe` (binary).  
`.qzi` as input: skips frontend, goes straight to backend. Binary emission also reuses the current project's `[cc]` and `[link]` inputs, so the same target-neutral QZI can be linked against the host's native C objects and libraries.
Linker: plain Linux and Windows binaries use in-process ELF/PE linkers with no
implicit native libraries. `--linker builtin` forces them. Archives/shared libraries,
`-l`, an explicit linker path, or `QUAZI_LINKER` opt into the external path
(`ld.lld`/`mold`/`ld` or `lld-link`/`link`); libc/CRT libraries are never added
implicitly. Target-compatible ELF/COFF objects passed to `qz build`/`qz run`
are linked by the built-in pipeline; archives, shared libraries, and `-l` remain explicit
external-linker features.
Rust edition 2024.

---

## Coding Rules

1. Write clean, maintainable, performant code.
2. Do not hardcode behavior that can be implemented directly in Quazilang.
3. Do not create useless attributes that contribute nothing — code that works without them is better.
4. Do not create excess intrinsics or attributes. Intrinsics are permitted when they are the cleanest or only viable choice; prefer stdlib code, but do not force awkward workarounds just to avoid an intrinsic.
5. Do not hardcode behavior.
6. Write all architectural changes to this file (AGENTS.md).
7. Aim not for the program just working, but for the program to be maintainable and clean.
8. No band-aid fixes. If a fix feels hacky, step back and redesign.
9. Keep the AST immutable after parsing; semantic analysis resolves meaning via annotations, not source mutation.
10. When fixing warnings, fix the root cause, not the symptom.

---

## Architecture

```
source → Loader → Lexer → Parser → Analyzer → Codegen → Backend (iced-x86) → .o → Linker → binary
```

QZI (`-b`): Codegen → serialized chunks, no backend.  
Object (`-c`): backend only, no linker.

### Component Guide

| Component | Path | Docs |
|-----------|------|------|
| Lexer | `src/lexer/` | [src/lexer/AGENTS.md](src/lexer/AGENTS.md) |
| Parser | `src/parser/` | [src/parser/AGENTS.md](src/parser/AGENTS.md) |
| Semantic | `src/semantic/` | [src/semantic/AGENTS.md](src/semantic/AGENTS.md) |
| Bytecode / Codegen | `src/bytecode/` | [src/bytecode/AGENTS.md](src/bytecode/AGENTS.md) |
| Backend (overview) | `src/backend/` | [src/backend/AGENTS.md](src/backend/AGENTS.md) |
| x86_64 Backend | `src/backend/x86_64/` | [src/backend/x86_64/AGENTS.md](src/backend/x86_64/AGENTS.md) |
| LSP | `src/lsp/` | [src/lsp/AGENTS.md](src/lsp/AGENTS.md) |
| Loader | `src/loader.rs` | (inline docs) |
| Project / manifest | `src/project.rs` | (inline docs) |
| Internal value layout | `src/runtime_layout.rs` | target-neutral Quazi VM slot shapes |

- The project is a single binary crate (`bin "qz"`) with inline `#[cfg(test)]` modules.
- No `tests/` integration directory yet — all tests are inline.

`runtime_layout.rs` is the source of truth for physical values crossing the
internal Quazi bytecode ABI. Resolve aliases and substitute generics before
querying it. Do not infer copyability from a one-slot layout: named aggregates,
enums, `String`, `fn`, and `dyn` are indirect handles with separate ownership
requirements (`MoveKind`). Concrete functions and generic specializations
record their parameter, variadic-element, and result layouts into
`SemanticReport::fn_value_layouts` (keyed by canonical resolved type
arguments) during analysis, and until the multi-slot ABI lands, storage
positions that still hold one slot per value — function signatures, enum
payloads, struct fields outside `@repr(C)`, and fixed-array elements — reject
multi-register shapes with `S14`. Qualified enum constructor calls
(`Option.Some(x)`) are validated in analysis (variant, arity, payload
representation) even though their result remains untyped until semantic
constructor resolution exists.

Generic call dependencies are closed over every concrete caller
specialization after analysis. This records each transitive concrete callee and
its value layout before bytecode emission, rather than falling back to a raw
generic template at codegen time.

### Loader (`src/loader.rs`)

- `load_programs` — resolves imports recursively, merges dependency-first, parses as one `Program`.
- Std resolution: compiler `CARGO_MANIFEST_DIR/std` → `~/.quazi/std` / `%USERPROFILE%/.quazi/std`.
- `foo/mod.qz` = opaque module directory; `pub import` controls what's exported.
- Deduplicates via canonical-path `HashSet`. Circular imports safe.
- **Namespacing**: every non-entry file gets module-qualified function names (`bar.foo`). Entry files keep bare names.

### Project (`src/project.rs`)

- `quazi.toml`: `[package]` including `out_dir = "build"`, `[lib]`, `[[bin]]`, `[dependencies]`, `[cc]`, `[link]`, and target link overrides. Dependencies support local projects, singular `.qz`, compiled `.qzi`, and internet `git`/`archive`/`source`/`qzi` sources. `quazi.lock` records exact revisions/checksums and is validated on build.
- `pub import` is the only public-import/re-export syntax. Quazi module and symbol paths use `.`, never `::`.
- QZI v8 is a sectioned executable/library bytecode container with package metadata, a public source interface, named call relocations, signedness-correct integer instruction flags, affine function-value ownership, layout-query intrinsic IDs, and legacy chunk payloads. Readers retain compatible QZI v2-v7 bytecode; v1 omitted frame metadata, parameterized v6 trait interfaces lack receiver metadata, and pre-v7 public function-value contracts or synthetic closure/forwarder chunks require a source rebuild. Immutable golden fixtures from real historical writers (`src/bytecode/fixtures/qzi/`) lock the v2-v6 reading paths, including the v2 chunk-header layout without a flags byte and v3 string-based `@api` metadata. QZI libraries work without original source; generic template bodies remain source-only for now.
- QZC v6 (`build/quazi/<target>/<artifact>/incremental.qzc` by default) stores exact-hit linked QZI plus source-hashed pre-WPO function chunks. Partial misses restore unchanged chunks, compile changed files, then rerun full-program WPO. V6 invalidates artifacts created before phase-2 layout intrinsic lowering and the current layout ABI boundary. Signature/type/import/config changes conservatively invalidate all chunks. Downloaded dependencies live in `<out_dir>/deps`.
- `type = "lib"` → lib project; default entry `src/lib.qz`; default output `.qzi`.

---

## Language Quick Reference

```quazi
import std.io.stdout;
import std.io as io;
pub fn name[T](param: Type, ...rest: str) ReturnType {
    const x: i32 = 1 + 2;
    var y: &str = "hello";
    var n: u64 = 42 as u64;
    x += 1; x -= 1; x++; x--;
    // Bitwise operators
    var b: u32 = x & 0xFF;              // & bitwise AND
    b = x | 0x01;                       // | bitwise OR
    b = x ^ 0x0F;                       // ^ bitwise XOR
    b = x << 2;                         // << left shift
    b = x >> 1;                         // >> right shift (sign-preserving)
    // Logical operators
    var ok: bool = true && false;       // && logical AND
    ok = true || false;                 // || logical OR
    ok = !ok;                           // !  logical NOT
    if (cond) { ... } else { ... }
    for (cond) { ... }                  // while-loop
    for i : 0..10 { ... }              // range loop
    for i : collection { ... }         // iterator loop
    for i, v : collection { ... }      // index+value
    // break; continue;
    var arr = [1, 2, 3]; arr[0];
    ret expr;
}
unsafe fn ptr_fn(p: *u8) *u8 { ret p; }
unsafe { var x = ptr_fn(p); *x = 1; }

// Entry point may take no args or a single Array[str].
fn main(args: Array[str]) i32 { ret args.len() as i32; }

struct Foo[T] { field: T, const flag: bool, }
trait Bar[T] { fn method(x: T) T; }
impl Bar[i32] for Foo[i32] { fn method(x: i32) i32 { ret x; } }
enum Option[T] { Some(T), None, }
match value { Some(v) => v, Option.None => 0, _ => default, }
match value { Some(v) if v > 0 => v, _ => 0, }   // guards

var f: fn(i32, i32) i32 = |x, y| x + y;   // closure
var g: fn() i32 = my_func;                  // fn-name as value

@repr(C)
type CCallback = fn(i32, i32) i32;          // raw C function pointer
// Only @export functions coerce to CCallback; invoking one is unsafe.
```

**Named arguments**: `foo(x=1, y=2)` — `name=value` pairs at call site. All positional args must precede named args.

Primitives: `i8/i16/i32/i64`, `u8/u16/u32/u64`, `isize`, `usize`, `f16/f32/f64`, `bool`, `str`, `bytes`, `void`. `any` is reserved for the compiler-erased final variadic parameter of `@format`; it is not a runtime value type. `b"..."` decodes byte escapes, while `br"..."` preserves them; byte strings are immutable, length-carrying, and expose `.len()`, indexing, and `.as_ptr()`.

### Unsafe System

- `*T` in fn signature → must be `unsafe fn` (S12). Exception: `@syscall`/`@api` implicitly unsafe.
- Calling `unsafe fn` or dereferencing `*T` outside unsafe context → S11.
- `@intrinsic` = safe (unsafety handled internally).
- `*T` ↔ `*U`: all raw pointers mutually compatible. Integer `0` valid as any `*T` (null pointer constant).

### String Model

- `str` and `String` use Unicode scalar indexes. `len()` counts scalars,
  `bytes_len()` reports encoded bytes, negative indexes count from the end, and
  `text[start:end:step]` follows Python slicing without splitting UTF-8.
  String comparison operators compare contents, not addresses. Case conversion
  is deliberately ASCII-only until Unicode tables are bundled.
- Checked `parse[T]()` supports signed, unsigned, pointer-sized, and floating
  primitives and returns `Result[T, ParseError]`. Search uses exact UTF-8 bytes
  and `find` returns a byte offset.
- `str` / `&str` — interchangeable. Immutable, valid UTF-8, fat pointer internally.
- `String` — owned heap string (`ptr+len+cap`). Local variables auto-clean via `String.free`.
- `Rune = u32` — Unicode codepoint.
- Quoted strings decode `\0`, `\a`, `\b`, `\e`, `\f`, `\n`, `\r`, `\t`, `\v`, punctuation escapes, `\xNN` ASCII escapes, one-to-three-digit octal escapes, C-style `\uNNNN`/`\UNNNNNNNN` and Rust-style `\u{H...}` Unicode scalar escapes, and escaped-newline continuations. Invalid escapes are lexer errors. Raw backtick strings decode nothing, may span lines, and must be terminated.

---

## Attribute System

| Attribute | Effect |
|-----------|--------|
| `@syscall("name"/num)` | Body → `Syscall+Ret`. Implicitly unsafe. |
| `@api("Symbol")` | Body → `CallExt+Ret`. Win64 on Windows, SysV elsewhere. Implicitly unsafe. |
| `@api` | Bodyless C ABI import using the Quazi function name as the native symbol. Every call requires `unsafe`; explicit `@api("Symbol")` is recommended. |
| `@api` on `var` | Imports a mutable C data symbol: `@api("symbol") var local: i32;`. Reads and writes require `unsafe`; only scalar, pointer, and C callback values are supported. |
| `@export("Symbol")` | Export an explicitly `pub` Quazi function under a stable C ABI symbol. Bare `@export` uses the function name. |
| `@repr(C)` | C-compatible struct/union layout and raw function-pointer aliases. Aggregates support by-value FFI, `packed`, and power-of-two `align=N`; empty and generic forms remain rejected. |
| `@opaque` | Declare an empty, non-generic foreign handle type which Quazi cannot construct. |
| `@cfg(key="val")` | Conditional compile. Keys: `target_os`, `target_arch`, `target_abi`. |
| `@inline` | Force inline eligibility (excluded if recursive). |
| `@ignore` / `@ignore(unused_vars)` / `@ignore(dead_code)` | Suppress W01/W02/W03/W07. |
| `@intrinsic("quazi.X")` | Safe stdlib wrapper; dispatched by encoder case number. |
| `@derive(Trait, ...)` | Register derived traits for struct. |
| `@panic_handler` | Validate signature; mark as panic handler. |
| `@test` | Mark a zero-argument `void` function for `qz test`. |

---

## Project Config (`quazi.toml`)

Minimal example:

```toml
[package]
name = "hello"
version = "0.1.0"
std = true
crash_handler = true
mangling = true

[build]
entry = "src/main.qz"   # optional, defaults to src/main.qz
src = "src"               # optional, defaults to src

[dependencies]
utils = { path = "../utils", version = "0.1.0" }

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

If a `quazi.lock` file exists, it is used to pin dependency versions. When missing and dependencies are present, a lockfile is created on build/run.

---

## Standard Library Status (`std/src/`)

| Module | Status | Notes |
|--------|--------|-------|
| `core` | Done | write/read/exit, memory/string primitives, numeric formatting, hostname/memory/CPUID and Windows release intrinsics, and an explicit raw string-pointer escape hatch |
| `io` | Done | println, print, eprintln, eprint, read_line — str_variadic |
| `fmt` | Done | `format(template, ...args: str)` — `{}` placeholders, spec-aware coercion |
| `string` | Done | UTF-8 `str`/`String`: rune lengths, Python slicing, content comparison, ASCII case conversion, search, generic checked parsing, and automatic cleanup |
| `panic` | Done | PanicInfo, __quazi_panic_handler, panic. Codegen injects file/line at call sites. |
| `result` | Done | ok/is_ok/is_err/unwrap/unwrap_err/unwrap_or; `?` operator |
| `option` | Done | is_some/is_none/unwrap/unwrap_or; `?` operator |
| `box` | Done | `Box[T]`: new, get, set, free |
| `traits` | Done | Display, Debug, Clone, Copy, Drop, Iterator, Eq, Ord, Hash, Default, Into, From, Index, Write, arithmetic traits |
| `prelude/mod.qz` | Done | re-exports String, Box, Array, option, result, traits, fmt, panic. Auto-injected always. |
| `collections/array` | Done | `Array[T]`: push, get, set, len, free, from, Index impl. Index assignment supported. |
| `collections/map` | Done | fallible, non-panicking `usize -> usize` open-addressing map; lookup returns `Option` |
| `collections/set` | Done | fallible, non-panicking `usize` open-addressing set |
| `unix` | Done | raw syscall wrappers |
| `windows` | Done | Win32 API wrappers |
| `fs` | Done | Cross-platform owned OS handles, automatic close, whole-file reads, paths, immediate entry counts, metadata, and mutation; Linux syscalls and Win32 APIs |
| `net` | Done | TcpListener, TcpStream, UdpSocket |
| `os` | Done | Cross-platform owned environment/hostname values, release/edition, CPUID branding, shell/terminal ancestry, memory totals, process control, cwd, sleep, and scheduling |
| `thread` | Done | spawn/join. No-capture only. |
| `ffi` | Initial | Cross-platform C aliases, `nullptr[T]()`, `CStr`, and checked `CString.try_from(bytes)`. |
| `math` | Done | Dependency-free integer combinatorics/GCD/LCM and lightweight f64 arithmetic, roots, trig, hyperbolic functions, exp/log, interpolation, and powers |

---

## Roadmap

### Philosophy
Fast binaries, small output, zero runtime waste. No LLVM, no GCC, no libc. `@intrinsic` → raw syscalls (Linux) or Win32 (Windows). QZI = stable portable IR.

### P0 — Critical Bugs / Safety

| Item | Status |
|------|--------|
| Enhanced Crash/Panic handler | ✅ Done |
| Fix encoder silent fallback | ✅ Done |
| Fix non-slice iterator codegen | ✅ Done |
| For-loop move semantics | ✅ Done |
| Index assignment | ✅ Done |
| Human-readable move errors | ✅ Done |
| **Module function namespacing/mangling** | ✅ **Done** |
| Mandatory safe indexing checks | ✅ Done — fixed arrays, slices, and bytes; unsafe C flexible arrays remain unchecked |

### P1 — High Impact

| Item | Status |
|------|--------|
| **Bitwise operators** | ✅ Done |
| **Loop control (`break`, `continue`)** | ✅ Done |
| **`else if` chains** | ✅ Done |
| **`unsafe` block sugar** | ✅ Done |
| **AOT `@cfg` stripping** | ✅ Done |
| **Built-in linker** | Experimental x86-64 ELF and PE32+ linking, cross-object symbols, checked relocations, generated Windows imports, and no implicit libc; archives pending |
| **`qz test` runner** | ✅ Done — discovers `@test` in `src/` and `tests/`, gives nested source files path-qualified module names and `tests/` a distinct `tests.*` root, compiles once, runs each test in an isolated process, reports infrastructure failures, and supports name filtering |
| **`pub` on types** | ✅ Done |
| **Unified formatting for `print`/`println`/`err`/`errln`/`format`** | In progress — support shared placeholder behavior, escaped braces, and format specifications; begin with `{:X}` and `{name:X}` uppercase hexadecimal |
| **Raw backtick string literals** | ✅ Done — contents are preserved exactly with no backslash escape decoding |
| **C/Rust-style escapes in non-raw strings** | ✅ Done — control, punctuation, ANSI `\e`, hexadecimal, octal, Unicode scalar, and line-continuation escapes with strict diagnostics |
| **C ABI FFI phase 1** | ✅ Initial — `@api`, `@export`, scalar/pointer signatures, `@repr(C)`, opaque handles, C compilation, object/library inputs, `.a`/`.so` output |
| **C ABI FFI phase 2** | ✅ Done — C variadics, scalar `f32`/`f64`, by-value `@repr(C)` aggregates, callbacks/function pointers, foreign globals, unions, packed/aligned structs, named integer bitfields, final flexible array members, portable byte strings, checked C-string construction, and target-aware C header generation for Linux SysV and Windows Win64 through portable QZI v5 metadata |

### P2 — Codegen Quality

| Item | Status |
|------|--------|
| Threshold-based auto-inline | ✅ Done |
| Cross-basic-block const folding | ✅ Done |
| Strength reduction | Pending |

### P3 — Platform / Runtime

| Item | Target |
|------|--------|
| macOS Mach-O backend | Actually implement Mach-O output, `__start` stub, relocations. |
| Ownership / RAII bytecode | In progress — WPO discovers `free` destructors and codegen inserts reverse-order lexical cleanup for locals and owned by-value parameters on fallthrough/early return, transfers returned ownership, and keeps method receivers borrowed. Explicit lifetime syntax, complete borrow checking, real `Move`/`Drop`/`Dup` lowering, trait dispatch, and recursive generic drops remain pending. |
| Atomic opcodes in encoder | `AtomicAdd`, `AtomicCas` lowering. |

### P4 — Language Features

| Feature | Status |
|---------|--------|
| Format string literals | Not started — `"value = {x}"` compile-time interpolation |
| Nested struct patterns | Not started — `Point { x, y }` in match arms |
| `async`/`await` | Not started — defer until lifetimes done |

### P5 — Tooling

| Component | Status |
|-----------|--------|
| LSP improvements | Partial — see [src/lsp/AGENTS.md](src/lsp/AGENTS.md) |
| `qz doc` | Not started |
| JIT VM | Deferred |

---

## Active Work Log

| Date | Change |
|------|--------|
| 2026-08-14 | Added `qz test [filter]` with `@test fn name() void`, project-wide source discovery, one shared compilation, isolated native runners, and readable summaries. Replaced source-level `@no_std`, `@no_crash`, and `@no_mangle`/`@no_mangling` controls with default-true `[package]` fields `std`, `crash_handler`, and `mangling`. |
| 2026-08-13 | Replaced exact-only QZC v1 with QZC v2: partial misses restore unchanged pre-WPO function chunks, compile changed-file functions, and rerun complete global WPO over the combined program; declaration/config fingerprints conservatively invalidate reusable units. |
| 2026-08-13 | Added first-class exclusive/inclusive `Range` expressions and OS-CSPRNG-backed `std.random` lowering with explicit availability failure and unbiased range selection. |
| 2026-08-13 | Added `[package].out_dir` with `build/` default, moved dependency materialization to `<out_dir>/deps`, added Git tag/hash/`latest` selectors, simplified `qz add` to one positional path/URL form, and made Git progress percentage-driven after the build header. |
| 2026-08-13 | Replaced filesystem-derived dependency-tree labels with stable logical import names and added diamond-based animated Git fetch/update progress. |
| 2026-08-13 | Made public declarations in a configured library entry point direct package exports under the manifest package name; child-file declarations still require explicit module imports or `pub import` re-exports. |
| 2026-08-13 | Replaced raw safe `std.net`/`std.fs` failures with portable `NetError`/`FsError` enums and readable messages; added minimal lowercase, timed CLI stages for cache lookup/write, frontend, parsing, analysis, bytecode, QZI linking, native emission, and linking; added `--silent`, `--no-progress`, `--no-color`, and `--no-unicode`; made `qz new`/`qz init` initialize Git and scaffold `.gitignore` files while keeping `quazi.lock` tracked; made `qz add <path-or-url>` infer package identity and support `--alias` without a redundant manifest identity field; added recursive factorial Git-dependency example 28; hid auto-prelude internals from the displayed file count; retained the former stdlib-DX topic as practical `27-text-and-math`. |
| 2026-08-13 | Split the HTTP example into launcher, server, and client artifacts so plain `qz run` prints safe instructions and `qz run --bin http-server` / `http-client` work on Windows/Linux without unsupported runtime-argument forwarding. |
| 2026-08-13 | Reorganized examples into a numbered, topic-named learning path; replaced regression-style output with practical programs; added full language, type/ownership, module/package, standard-library, and FFI documentation indexes. |
| 2026-08-13 | Corrected QZI v6 emission so executable projects store no source-library interface and library interfaces exclude dependency generics before validation. Added Winsock-backed `std.net`, automatic `ws2_32` linkage, Windows socket error preservation, package-command regression coverage, and a full Windows check/build/run example matrix. |
| 2026-08-13 | Added transactional `qz add`/`qz remove` manifest and lockfile updates, relative path dependency examples, and an HTTP client/local-server example. Expanded `std.net` with complete sends, bounded receives, HTTP requests, response encoding, and one-request serving. |
| 2026-08-11 | Fixed Windows interactive input so CRLF/CR Enter becomes `\n` instead of returning the cursor to column zero; changed `std.io.read` from numeric bytes to non-empty UTF-8 string delimiters, including multi-byte sequences. Documented `str`/`&str`/`String`, demonstrated multiline raw strings, and made unterminated raw literals a lexer error. |
| 2026-08-11 | Made custom-delimiter and single-key terminal reads immediate on Windows/Linux while preserving redirected and line-buffered input. Hardened Win64 `ReadFile`/`WriteFile` lowering so result counts live outside shadow space and API failures return `-1`; stopped RAII from destroying uninitialized owned locals before their first assignment. |
| 2026-08-10 | Expanded primitive DX with rune-based `len`, `bytes_len`, negative indexing, Python-style slices, content comparison operators, generic checked `parse[T]()`, numeric methods, broader dependency-free `std.math`, and `examples/22-stdlib-dx`; made Windows console output UTF-16-safe while preserving redirected UTF-8. Fixed initialized-binding aliasing, incomplete jump-aware inlining, float comparison/negation lowering, and function-table hole compaction found by the executable checks. |
| 2026-08-10 | Restored `21-quazifetch`'s Unicode frame by default with an executable `-a`/`--ascii` fallback and made Linux package counting linear instead of repeatedly rescanning the status buffer. Fixed semantic constant tracking so assignments to mutable variables invalidate compile-time values instead of incorrectly collapsing runtime branches, and made Windows `main(args: Array[str])` allocations use the process heap to match automatic `core.free` cleanup without importing the CRT. |
| 2026-08-10 | Corrected `21-quazifetch` system identity and package reporting: normalized hostnames, added CPUID CPU branding, unmanifested Windows build/edition detection, Toolhelp shell/terminal ancestry, Explorer labeling, and counted package metadata. Added automatically-owned cross-platform directory enumeration through `getdents64`/Win32 find handles and moved temporary stdlib buffers to destructor-backed ownership adapters. |
| 2026-08-10 | Made `21-quazifetch` shell-free and portable across Windows/Linux using cross-platform `std.fs`/`std.os`; added hostname and memory intrinsics, kernel-provided Linux environment access, pointer-correct Win32 filesystem bindings, and CRT-free Windows allocation/string paths used by the example. Extended WPO-driven RAII so owned by-value parameters clean up on fallthrough and early returns without suppressing sibling-path cleanup, while returns transfer ownership and method receivers remain borrowed. |
| 2026-08-10 | Documented the built-in linker as a public contract in `docs/LINKER.md` and the README: selection/fallback rules, source/QZI/object workflows, ELF symbol/relocation and segment guarantees, embedded runtime behavior, explicit libc opt-in, diagnostics, and experimental limitations. Enforced the documented relocatable-object and allocated-section checks. |
| 2026-08-10 | Started the experimental self-hosted linker: added an in-process static x86-64 ELF writer with checked relocations, W^X load segments and a non-executable stack; made native libraries and libc explicit; removed Linux startup's libc environment lookup; and embedded on-demand mmap-backed allocation/memory routines in compiler objects. |
| 2026-08-10 | Hardened QZI generation/loading with checked encoding limits, malformed-input and ABI validation, shared register accounting, and error-returning backend fallbacks. Widened field offsets to 16 bits, preserved `?` payload types for method dispatch, fixed directory-backed lazy re-exports and imported static constructors, corrected Win64 intrinsic stack/shadow-space handling and 64-bit formatting, and tightened standard-library I/O/string/collection ownership contracts. |
| 2026-08-10 | Completed the example compile matrix: made sibling gateway imports explicitly relative to avoid package-name shadowing, accepted CRLF string continuations, stopped target-neutral QZI builds from invoking native C tools, and normalized Windows extended paths before invoking external compilers/linkers. |
| 2026-08-06 | Made the C ABI phase-two example self-explanatory on stdout: it now introduces the test, reports each of its nine checks as PASS or FAIL, explains nonzero exit codes at the failure site, and prints a final success summary. |
| 2026-08-06 | Fixed native callback-address linkage by defining each export adapter under both its stable C symbol and its compilation-local synthetic symbol. This preserves existing portable QZI callback relocations on ELF and COFF. |
| 2026-08-06 | Added `qz header` for deterministic C/C++-compatible declarations of `@export` functions and their `@repr(C)` dependencies. It supports callbacks, unions, packed/aligned structs, bitfields, flexible array members, aliases, opaque handles, target `@cfg`, and Linux/Windows C data models without compiling or linking. |
| 2026-08-06 | Added mutable foreign globals with `@api("symbol") var name: Type;`. Scalar, pointer, and C callback reads/writes require unsafe context and lower through portable QZI v5 data-symbol metadata to ELF/COFF PC-relative relocations. Macro and TLS pseudo-globals such as `errno` remain accessor functions. |
| 2026-08-06 | Added raw C callbacks through `@repr(C) type Callback = fn(...) ...`; `@export` functions coerce to callback pointers, `@api` may accept or return them, and indirect calls lower through the target SysV/Win64 ABI. Callback invocation is unsafe and ordinary Quazi closures cannot cross the C boundary. |
| 2026-08-06 | Extended C aggregate layouts with `union`, `@repr(C, packed, align=N)`, named nonzero integer bitfields, and pointer-only final `[T; ..]` flexible array members. Union field access and flexible-array indexing are unsafe. |
| 2026-08-06 | Added immutable `bytes` with `b"..."`/`br"..."`, exact QZI v4 byte constants, `.len()`/indexing/`.as_ptr()`, and checked `std.ffi.CString.try_from(bytes)` with interior-NUL and allocation errors. |
| 2026-08-06 | Extended `qz run` to accept the same QZI, source, C, Unix/Windows object-library, library-path, and library inputs as `qz build -r`; invoking it without files retains current-project mode through `quazi.toml`. |
| 2026-08-06 | Added portable QZI v3 C ABI signatures and symmetric adapter chunks for `@api`/`@export`; implemented scalar float and by-value `@repr(C)` aggregate arguments/returns for Linux SysV and Windows Win64, including hidden sret, stack overflow arguments, C variadic promotions/SSE counts, `f32` field conversion, and Windows native-source/linker support. Added `examples/20-ffi-abi`. |
| 2026-08-03 | Added the first Linux x86-64 C ABI layer: unified `@api` imports, stable `@export` symbols, scalar/pointer signature validation, `@repr(C)` scalar/pointer layouts, opaque handles, `$CC` native sources, object/archive/shared-library linkage, static/shared outputs, `std.ffi`, and a C→Quazi→C round-trip example. Unsupported ABI cases fail explicitly instead of guessing. |
| 2026-08-02 | Expanded quoted-string escapes with C control/octal forms and Rust hexadecimal/Unicode forms. Invalid escapes now produce lexer diagnostics; raw strings preserve every escape spelling. |
| 2026-08-02 | Removed the nested `quazistrap/std` checkout. Std resolution now checks the compiler's Cargo manifest directory for `std/`, then the user installation at `~/.quazi/std`; prelude module headers no longer identify themselves as `std.*`. |
| 2026-06-07 | Module function namespacing/mangling implemented. Non-entry files prefix top-level functions with module name (`bar.foo`). Entry files keep bare names. `import bar.foo` errors on collision with local fn. `import bar.foo as b_foo` aliases cleanly. All 137 tests pass. |
| 2026-06-11 | Fixed canonical path mismatch in loader that could cause entry files to be namespaced. Added `@no_mangle` attribute: keeps function symbol name bare (no module prefix). All 146 tests pass. |
| 2026-06-11 | Implemented `fn main(args: Array[str])` support. Semantic analysis validates the parameter signature and sets `SemanticReport.main_takes_args`. Linux startup stubs build an `Array[str]` from `argc`/`argv` and pass it in `rdi`; Windows stubs use `__getmainargs` to obtain parsed argv and build the same array. Added `examples/13-args`. All 151 tests pass. |
| 2026-06-11 | Hardened slice support: `types_compatible` now rejects fixed-size array ↔ slice coercion, which previously generated invalid code and crashed at runtime. Added a clear `S08` diagnostic for this case. `for item : items` over variadic slices continues to work. Full array-to-slice coercion remains on the roadmap. All 152 tests pass. |
| 2026-07-26 | **Rebranding**: Binary renamed `void` → `qz`. Config files `quazilang.toml`/`quazilang.lock` → `quazi.toml`/`quazi.lock`. Internal ABI symbols `__void_*` → `__quazi_*`. Intrinsic namespace `void.X` → `quazi.X`. All docs merged from AGENTS.md + CLAUDE.md into AGENTS.md. |
| 2026-07-27 | Documented logical operators (`&&`, `\|\|`, `!`) — all were already fully implemented in the compiler (lexer/parser/semantic/codegen). Added `examples/15-logical` demonstrating all three operators with a truth-table program. Updated Language Quick Reference and Examples table. |
| 2026-07-27 | Implemented `pub` visibility enforcement on types (`struct`, `enum`, `trait`, `type`). Imported `AST` types now carry a `public` flag. Semantic analysis emits an `S04` error when attempting to import a non-public type across modules. Updated standard library (prelude) types like `Array` to be `pub`. |
| 2026-07-27 | Implemented cross-basic-block constant propagation (`const_prop_fold`) in the bytecode optimizer. Constant folding operates on integers and floats natively, folding mathematical sequences and eliminating dead branches at compile-time. Added `17-constfold` example. |
| 2026-07-29 | Fixed raw-pointer dereferences to honor integer pointee widths. QZI `Load`/`Store` flags now carry byte/word/dword/qword width metadata; signed sub-word loads sign-extend, unsigned loads zero-extend, and legacy zero flags remain qword-compatible. Explicit dereference reads, writes, and compound assignments are covered by codegen tests. |
| 2026-08-05 | Implemented C variadic FFI (Phase 2): bare `...` in `@api` parameter lists now compiles to a C-variadic call. Parser detects `...` immediately before `)` as a C-variadic marker (`c_variadic: bool` on `ItemKind::Fn`). Semantic analysis lifts the S14 error for bare `...` while still rejecting Quazi-style `...name: T` variadics in FFI. Call-site ABI metadata now records every promoted actual argument; SysV sets `AL` to the used SSE-register count and Win64 duplicates variadic floats in positional integer registers. Added `std.ffi.va_list` opaque type and `examples/19-cvariadics`. |

---

## Examples

The authoritative 28-project user curriculum is
[examples/README.md](examples/README.md). Historical work-log references above
record old names at the time of each change; they are not current paths.

Native FFI artifacts support target-specific Windows `.dll`, Linux `.so`, and
`qz header` `.h` output. Shared libraries omit process startup and expose only
explicit `@export` symbols through an external linker. `std.dylib` provides
runtime DLL/SO loading; raw symbol casts remain unsafe.
