# Workspace baseline audit — 2026-08-29

Audience: language maintainers and tooling developers.

Status: milestone 1 in progress. This report records confirmed evidence; it is
not a claim that the language or ecosystem is production-ready.

## Repository inventory

| Project | Role | Git state at audit | Evidence |
|---|---|---|---|
| `quazistrap/` | Bootstrap compiler, runtime lowering, prelude, contained LSP, canonical docs | `feat/test`, two local commits ahead, with preserved compiler/test-runner changes | independent `.git`, `Cargo.toml`, `src/`, `prelude/`, `docs/` |
| `std/` | Standard library | clean `main` | independent `.git`, `src/mod.qz` |
| `tree-sitter/` | Intended canonical Tree-sitter grammar | clean `main`, but still identifies as Void | independent `.git`, `grammar.js`, `tree-sitter.json` |
| `parse_ini/` | Local development library | repository with no commits; files untracked | independent `.git` |
| `ini_test/` | Local INI consumer | repository with no commits; files untracked | independent `.git` |

The workspace-root `.git/` is empty and is not a usable repository. No VS Code,
Zed, Neovim, or Helix integration project currently exists under
`~/Projects/qz`.

## Verification baseline

- `cargo test --offline` in `quazistrap`: 375 passed after the checked-indexing
  and signedness/QZI v7 safety checkpoints.
- `qz test` in `examples/32-testing`: 6 source-level native tests passed,
  including checked fixed-array/slice/bytes boundaries, full-block copies,
  high-bit unsigned arithmetic, and signed overflow/shift edges.
- INI source-dependency example: builds, runs, and round-trips files exactly.
- `npm test --offline` in `tree-sitter`: cannot start because the local
  `tree-sitter` executable/dependencies are absent.
- `std` has no standalone manifest or test runner. Compiling `src/mod.qz`
  directly does not reproduce resolver context and yields privacy/import errors,
  so it is not a valid library verification method. Current evidence comes from
  compiler tests and consuming projects, leaving a systematic test gap.

## Prioritized confirmed findings

### P0 — Type compatibility permits representation-unsafe substitutions

Original evidence: `src/semantic/typecheck.rs` contained a broad compatibility
arm that accepted any `Named` type against every other type. This accepted borrowed `str`
where owned three-word `String` was required and accepted `&String` where
`String` was required. The resulting native program dereferenced string bytes as
a struct and crashed.

Impact: silent ABI/type corruption, crashes, and potential memory unsafety across
assignments, arguments, returns, generic calls, and collection elements.

Intended behavior: only identical named constructors with recursively compatible
type arguments, approved trait-object coercions, and explicit representation-safe
conversions are compatible. `str` to `String` must allocate explicitly.

Compatibility: tightening exposes programs that compiled accidentally. This is a
necessary safety break and needs a migration entry describing `String.from(text)`.

Verification: negative semantic tests for unrelated named types, `str`/`String`,
references, and generic arguments; positive tests for identical and substituted
named types; native regression that previously crashed.

Checkpoint status: the universal named-type arm has been removed. Generic
instances are invariant except for representation-identical `str`/`&str` views,
generic struct literals infer arguments, zero-payload `None` is handled
explicitly, and `Option`/`Result` receiver substitution is registered. Focused
and full offline compiler suites pass. Remaining `any`, reference, and raw
pointer holes below prevent declaring the type system sound.

### P0 — `any` is an untagged representation escape

Evidence: compatibility treats `any` as universally interchangeable, while the
runtime has no dynamic tag or checked coercion. An integer can therefore flow
into a pointer- or string-shaped destination without a runtime check.

Impact: implicit downcasts can reinterpret arbitrary bits as addresses.

Intended behavior: distinguish compiler inference variables from runtime
dynamic values. Either make `any` a tagged value with checked downcasts or
require explicit, validated conversions.

Checkpoint status: the compiler now reserves source `any` and rejects it in
all value-bearing positions. Internal error recovery has a distinct
non-source `Error` type, generic instances are invariant, closures require
contextual function signatures, dynamic trait calls preserve their declared
signatures, and code generation rejects any representation-less type that
survives analysis. The only exception is an `@format` function's final
`...args: any` pseudo-parameter, which is erased after call-site formatting.
Public QZI interfaces defensively reject runtime `any`. `std.thread` now uses
exact target-specific C callback aliases instead of an untyped function slot;
Linux spawn failures clean up and return zero, and joining zero is a no-op.
QZI rejects public generic methods without template bodies, imports impl-only
interface modules, and fails ambiguous parameterized or runtime-`any` v6
interfaces with rebuild guidance. Trait implementations cannot weaken a safe
trait method into an unsafe implementation. Regression coverage includes
semantic compatibility, constructor inference, trait conformance and object
calls, closure context across callable routes, QZI
export/materialization validation, and Linux/Windows thread object compilation.
The post-review checkpoint passes 410 offline compiler tests and all 6 tests in
the real testing example.

### P0 — References have no lifetime or address-taken model

Evidence: references may target temporaries or escape a function. Codegen forms
them from frame-slot addresses, while register allocation may recycle the
pointee slot before the reference dies. Raw pointers are also implicitly
compatible with safe references, and shared references permit stores.

Impact: dangling pointers, use-after-reuse inside one scope, and bypass of
unsafe/mutability rules.

Intended behavior: add address-taken slot metadata and lexical/escape checking,
separate shared and mutable references, and prohibit raw-to-safe conversion
outside an explicit checked unsafe operation.

Checkpoint status: resolved with a deliberately conservative lexical model.
Values cannot become references, pointee types are invariant, and address-of is
limited to local/parameter places until field/index address lowering exists.
Non-string references cannot be returned, stored in owned aggregates, rebound,
or captured by closures. Address-taken owners cannot be mutated, moved, or used
as method receivers during the remaining function-local borrow. Scalar `Lea`
records a one-slot address-taken block so register allocation cannot recycle the
pointee, QZC v3 invalidates stale incremental artifacts, and QZI inputs lacking
explicit `Lea` metadata fail with source-rebuild guidance. `str`/`&str` remains
the representation-identical string-view exception; raw-pointer escape remains
explicitly unsafe.

### P0 — Bounds and unsigned numeric lowering are unsafe

Evidence: fixed arrays and slices lower indexing directly to pointer arithmetic
without bounds checks. Unsigned comparisons and division use signed machine
operations. A high-bit `usize` can therefore pass a source-level bounds check
and reach an out-of-bounds access.

Impact: memory corruption from safe-looking indexing and incorrect arithmetic.

Intended behavior: make signedness/width part of the bytecode contract, lower
unsigned comparisons/division correctly, and insert mandatory checks for safe
fixed-array and slice indexing.

Checkpoint status: resolved. QZI v7 records unsigned division, remainder, and
relational semantics; semantic folding, bytecode constant propagation, and the
SysV/Win64 backend agree; signed and unsigned right shifts select arithmetic
and logical instructions respectively. Safe fixed-array, slice, and bytes
indexing now has mandatory unsigned bounds guards, including writes and
compound assignments. Constant invalid indices are semantic errors. Explicit
`Lea` block-length metadata preserves fixed-array register contiguity through
both allocation passes. Failure uses the language panic path or a deterministic
native trap in freestanding builds. Unsafe C flexible arrays intentionally
remain unchecked. Representation-unsafe fixed-array parameters and returns are
rejected until a borrowed aggregate ABI is defined. Compatible QZI v2-v6
bytecode remains readable; v1 and ambiguous parameterized v6 trait interfaces
fail with source-rebuild guidance instead of being guessed.

### P0 — Closure environments leak and owned captures can dangle

Evidence: every closure/function value allocates an environment that is never
destroyed. Captures are shallow stores, and captured owned locals remain eligible
for ordinary scope cleanup.

Impact: leaks for every function value and use-after-free for escaping closures
that capture owners.

Intended behavior: define closure environment ownership, move/borrow capture
rules, and recursive destruction before stabilizing closures or concurrency.

Status (2026-08-29): resolved conservatively. `fn` values are affine owners;
scope exit, replacement, discarded temporaries, immediately called temporaries,
and consumed parameters free exactly one environment. Returns transfer the
environment to the caller. Closure chunks reserve the hidden environment and
all user parameter registers before loading captures, nested symbols are unique,
and named-function forwarders are deduplicated. Until recursive cleanup exists,
captures and closure ABI values are limited to immutable plain scalars, and
owned function values cannot be nested in aggregates or generic arguments.
Conditional moves and consuming assignment expressions are rejected until
cleanup is path-sensitive. QZC v4 invalidates stale closure chunks, and QZI v7
rejects pre-ownership callable contracts or synthetic closure chunks from older
artifacts.

### P0 — Generic `Array[T]` and `Box[T]` only store one machine word

Evidence: `prelude/src/array.qz` allocates and indexes `8` bytes per element and
routes values through one-register intrinsics. `prelude/src/box.qz` does the
same for one value. Fixed arrays and slice-like multi-register values are
therefore truncated. Ordinary structs, enums, and `String` are instead
represented internally by one-word heap handles, so their bytes are not
truncated; their ownership is still unsound. `get` creates a shallow owner
alias, `set` overwrites without destroying the old element, and container
`free` destroys only the backing allocation. `std.net.Headers` uses
`Array[Header]`, where each `Header` contains owned strings.

Impact: truncation for genuine multi-register values; aliasing, missing
destructor traversal, leaks, future double frees, and use-after-free for owned
handle values; and crashes in APIs advertised as safe and generic.

Intended behavior: generic storage must use compiler-known size/alignment and
well-defined move/drop operations, or APIs must be restricted to one-word copy
types until that machinery exists.

Compatibility: a sound implementation may change layout and QZI ABI. Restricting
types is an immediate safety break. Maintainer decision D-001 is required.

Checkpoint status (2026-08-29): the compiler now models internal runtime value
shapes and revalidates every concrete generic function/method parameter,
variadic element, and result. Multi-register specializations fail with `S14`
before code generation, and QZC v5 invalidates cached bytecode that predates the
check. This closes silent truncation but does not resolve owned handle access or
recursive destruction; the P0 remains open pending D-001 and D-003.

Verification: fixed-array and slice-like element tests, nested owned handles,
growth/reallocation, replacement, removal, early return, and exact destructor
counts under native and bytecode paths. A native probe confirmed that an
`Array[Triple]`/`Box[Triple]` round-trip currently preserves the heap handle,
while `Array[[i32; 3]]` loses elements after the first register.

### P0 — Collection update idiom conflicts with compiler ownership

Evidence: `Map.insert` and `Set.insert` return shallow structs sharing buffers.
Documented use assigns the result back to the original. Assignment codegen drops
the old owned local before installing the returned value, while method receivers
are non-consuming.

Impact: use-after-free and double-free on ordinary map/set mutation.

Intended behavior: mutating methods should update the receiver in place and
return `Result[void, E]`, or ownership-consuming receivers must be represented
and enforced explicitly. Shallow owner copies are invalid.

Compatibility: changing return types is source-breaking; changing receiver move
semantics is language-wide. Decision D-002 is required.

Verification: native insert/grow/replace/remove tests under allocator poisoning,
early return, and scope cleanup; compiler ownership tests for assignment from a
receiver-derived result.

### P0 — Owned resource cleanup is shallow and explicit cleanup can repeat

Evidence: codegen only searches for `.free` on the exact named local type and
does not recursively destroy owned fields. Network aggregates containing
`String`, `Array`, or socket owners define no aggregate destructor. `CString.free`
does not invalidate its pointer, while callers also remain eligible for automatic
scope cleanup.

Impact: leaks for composite values and possible double-free after explicit
cleanup. Documentation currently promises broader RAII behavior than exists.

Intended behavior: define one coherent move/drop model with field destruction,
idempotent early close, and ownership transfer. User-defined `free` must not be
treated as a sufficient substitute without exact lifecycle rules.

Compatibility: destructor insertion changes observable cleanup timing and may
surface invalid shallow copies. Decision D-003 is required before broad changes.

Verification: destructor-order/count tests for nested fields, reassignment,
returns, branches, loops, explicit close/free, and panic/termination paths.

### P0 — Text APIs promote unchecked bytes into valid UTF-8 strings

Evidence: `std.fs.read_to_string` and network receive paths terminate arbitrary
bytes and pass them to unsafe `String.from_raw` without UTF-8 validation, while
language documentation defines `str`/`String` as valid UTF-8.

Impact: later Unicode indexing can read invalid sequences or past buffers;
untrusted file/network input violates a core type invariant.

Intended behavior: binary reads return bytes; text reads validate UTF-8 and
return a structured decoding error.

Compatibility: error enums and signatures may need new variants. Existing
invalid-byte inputs change from unsound success to error.

Verification: malformed, truncated, overlong, surrogate, maximum-scalar, and
valid multilingual fixtures on file and local socket paths.

### P0 — Tree-sitter describes another language

Evidence: the canonical separate repository uses grammar name `void`, package
`tree-sitter-void`, scope `source.void`, and `.void` files. It accepts legacy
`while` and omits current Quazi constructs including `union`, aliases,
`break`/`continue`, bytes, inclusive ranges, bitwise operators, closures, named
arguments, and current string forms.

Impact: editor parsing/highlighting cannot be considered Quazi support.

Intended behavior: rename the existing repository in place and derive a syntax
matrix from the compiler parser, specification, tests, and examples.

Compatibility: package and node-type identities change. No published migration
is performed locally, but bindings and future editor consumers must update
together.

Verification: Tree-sitter corpus for valid, invalid, ambiguous, and incomplete
programs plus a conformance job parsing every checked-in `.qz` example.

### P0 — LSP position and document models are not protocol-correct

Evidence: `src/lsp/span.rs` mixes compiler character offsets, UTF-8 byte offsets,
and LSP UTF-16 units. Formatting uses byte length for the terminal character and
often a one-past-last line. Analysis is single-buffer; goto-definition returns
the first same-name symbol in the current URI. No close handling or stale-version
guard exists.

Impact: wrong diagnostics, hover, edits, and navigation for Unicode; stale or
cross-file-invalid results in real editors.

Intended behavior: one tested UTF-16 conversion layer, versioned document state,
project-loader-backed analysis, and scope-aware semantic identities.

Compatibility: protocol correctness only; clients may observe corrected ranges
and capabilities.

Verification: astral/BMP Unicode position tests, full-document edits with and
without final newline, out-of-order changes, close/reopen, incomplete source,
multi-file projects, and JSON-RPC lifecycle tests.

### P1 — Standard-library platform and error contracts overstate behavior

Confirmed examples:

- Linux TCP send uses `write(2)`, permitting process-killing `SIGPIPE` instead
  of returning `NetError`.
- Portable `std.thread` and `std.os` functions call Unix APIs unconditionally.
- Formatting returns `str` with mixed borrowed/heap behavior and deliberately
  clears the only owner, leaking formatted allocations; `Display.to_string`
  claims ownership but returns `str`.
- Custom panic-handler validation accepts ABI-incompatible named/string
  parameters and returning handlers despite the runtime passing `PanicInfo` and
  panic being non-returning.

Each needs a focused contract, regression, implementation fix, and change or
migration note before API expansion.

### P1 — Bytecode and low-level ABI validation is incomplete

Confirmed examples include `CallExt` accepting a constant-pool entry of the
wrong kind, allocation opcodes proceeding after a null result, and syscall
declarations previously encoding more arguments than the backend can load.

Checkpoint status: syscall declarations now reject more than six parameters,
generic signatures, and non-register ABI types; focused and full compiler tests
pass. `CallExt`, `CallCReg`, and syscall instructions now reject constant-pool
metadata of the wrong kind before serialization or backend lowering.
Deterministic allocation failure remains open.

### P1 — Documentation, site, LSP, and editors are incomplete

Canonical documentation is inside `quazistrap/docs`, but it was flat and lacks
the required specification/API/tutorial/guides/internals/tooling/changes/
migrations/site separation. There is no local documentation-site pipeline,
search, link checker, version strategy, or tested Markdown example harness.

The contained LSP lacks signature help, references, rename, document/workspace
symbols, semantic tokens, incremental sync, cancellation, workspace loading,
and general completion. Its nine focused tests do not exercise protocol or real
client behavior.

All four requested editor projects are absent. Neovim 0.12.5 is locally
available; VS Code CLI, Zed, and Helix are not currently in `PATH`, so those will
need deterministic package fixtures unless tooling becomes available.

## Roadmap challenge and sequencing

Do not expand `time`, process management, TLS, concurrency, or editor feature
surface before the P0 representation/ownership/text invariants are resolved.
Otherwise new APIs multiply unsound owners and invalid generic aggregates.

Recommended order:

1. Freeze and test type compatibility, value layout, move/drop, UTF-8, panic,
   and supported-platform contracts.
2. Establish native regression harnesses for memory/resource failures and a real
   standard-library project test runner.
3. Repair generic storage or temporarily constrain it, then repair dependent
   networking and formatting APIs.
4. Establish canonical specification/API/change/migration documentation and
   automated example/link checks.
5. Align Tree-sitter to the frozen grammar; then make LSP positions, document
   state, and project analysis correct.
6. Build editor integrations on those canonical parser/LSP contracts.
7. Only then extend `time`, `process`, TLS-backed networking, and concurrency.

## Security and release readiness

- Invalid UTF-8, representation confusion, double-free/use-after-free, SIGPIPE,
  shell/process design, TLS verification, and unbounded network/file operations
  are security-relevant and require hostile-input tests.
- Supported targets are not yet stated as a compatibility contract. Current
  evidence centers on x86-64 Linux and Windows; macOS enum/backend paths exist
  but are incomplete.
- No language stability/versioning policy, deprecation window, MSRV/editor
  support matrix, release checklist, or compatibility guarantee is canonical.
- Production-ready status is therefore contradicted by current evidence.

## Verification gaps

- No sanitizer/Valgrind-style native memory suite.
- No malformed-input fuzz/property suite for lexer/parser/QZI/linkers.
- No independent standard-library test project.
- No cross-platform CI evidence in the local workspace.
- No parser/Tree-sitter/documented grammar conformance test.
- No LSP protocol/client smoke suite.
- No documentation build, tested-snippet system, or link checker.
