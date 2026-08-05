Implement the following changes to the Quazilang compiler (quazistrap), in exact priority order. Do NOT touch LSP — it is lowest priority. Update AGENTS.md as you go.

  ## P0 — Critical Safety / Bugs

  1. [DONE] Enhanced Crash/Panic Handler
     - Current: Linux `__quazi_crash_handler` and Windows `__quazi_crash_handler_win` print a static 78-byte message with no context.
     - Goal: include (a) the panic message string if available, (b) file/line info from the panic site, (c) optional stack trace via rbp frame walking when `QUAZI_TRACE=1` is set.
     - Files: `src/backend/x86_64/start.rs`, `src/backend/x86_64/sections.rs`, `std/src/panic.qz`, `std/src/core.qz`.
     - Keep binary size minimal; use compile-time-known offsets where possible.

  2. [DONE] Fix Encoder Silent Fallback
     - Current: in `src/backend/x86_64/encoder.rs` around line 1607, unimplemented opcodes (`NewObj`, `Move`, `Drop`, `Dup`, `Spawn`, `AtomicAdd`, `AtomicCas`, `StrConcat`) silently emit `xor rax,rax; mov slot
  (0),rax` — producing wrong code.
     - Fix: replace silent fallback with `panic!("unimplemented opcode in encoder: {:?}", op)` or return a proper `Err`. Wrong code is worse than a crash.

  3. [DONE] Fix Non-Slice Iterator Codegen
     - Current: in `src/bytecode/codegen.rs` around line 1624, `ForLoop::Each` on non-slice collections emits a broken infinite loop (`Jz` + `Jmp` to top with no `.next()` call).
     - Fix: since `Iterator[T]` trait exists (`std/src/prelude/traits.qz`) with `next()` and `has_next()`, emit proper iterator protocol calls: bind iterator result, loop while `has_next()`, call `next()` to
  get value. Do not change the AST; fix only the codegen path.

  ## P1 — High Impact

  4. [DONE] Bitwise Operators (`&` `|` `^` `<<` `>>`) and Logical Operators (`&&` `||` `!`)
     - Problem: these are not parsed as binary operators. The encoder already implements `And`, `Or`, `Xor`, `Shl`, `Shr`, `Sar`. The stdlib already defines `BitAnd[T]`, `BitOr[T]`, `BitXor[T]`, `Shl[T]`, `Shr
  [T]` traits. But the parser has no `BinOpKind` variants for them, and codegen's fallback for unknown binary ops silently emits `Add` instead.
     - Lexer: add `Caret` (`^`), `Shl` (`<<`), `Shr` (`>>`) tokens. `&` and `|` already exist as `Ampersand` and `Pipe`.
     - Parser (`ast.rs` + `mod.rs`): add `BitAnd`, `BitOr`, `BitXor`, `Shl`, `Shr` to `BinOpKind`. Add precedence levels between comparison and term:
       - `parse_shift` → `<<` `>>`
       - `parse_bitwise_and` → `&`
       - `parse_bitwise_xor` → `^`
       - `parse_bitwise_or` → `|`
       - Order: logical-or → logical-and → bitwise-or → bitwise-xor → bitwise-and → equality → comparison → shift → term → factor
     - Parser ambiguity: `&` prefix = reference (unary), `&` infix = bitwise AND. `|` primary = closure start, `|` infix = bitwise OR. Ensure primary-position `Pipe` still starts closures.
     - Semantic (`typecheck.rs`): typecheck bitwise ops to integer types (bool allowed for `&`/`|`/`^` if desired).
     - Codegen (`codegen.rs`): map each new `BinOpKind` to its `Opcode` (`And`, `Or`, `Xor`, `Shl`, `Shr`). Replace the `_ => Add` fallback with a compile error / panic for unhandled ops.
     - Optional: compound assignment variants (`&=`, `|=`, `^=`, `<<=`, `>>=`) if trivial to add.

  5. [DONE] AOT `@cfg` Stripping
     - Current: `@cfg` conditions are evaluated in semantic passes (declare, typecheck, unused) but dead `CfgBlock` AST nodes are still passed to codegen.
     - Fix: add a pre-codegen AST pruning pass that removes statement-level `CfgBlock` nodes whose condition is false. This should happen after semantic analysis but before `compile_program`. Also strip `@cfg`
  -disabled top-level items from the program item list so codegen never sees them.

  6. `qz link` Built-in Linker
     - Goal: when `qz build myprog.o` is invoked (an object file as input), use a built-in minimal linker to produce the final binary instead of calling the system linker.
     - For ELF (Linux): parse the existing `.o` (or produce it internally), emit a single ELF executable with `.text`, `.rodata`, `.data`, `.bss`, entry point `_start`, and `PT_LOAD` segments. Target <500B for
  a hello-world binary.
     - For PE-COFF (Windows): emit a minimal PE with `.text`, `.rdata`, `.data`, entry `mainCRTStartup`, and correct headers. Target <700B.
     - Reuse existing `src/backend/x86_64/start.rs` stubs. Reuse existing section/symbol/relocation logic where possible.
     - The built-in linker should be triggered automatically when the input to `qz build` ends in `.o` and the user did not pass `--obj`. It should also be usable as `qz build --link myprog.o` if that is c
  leaner for the CLI.
     - Update `src/cli.rs` and `src/main.rs` command routing as needed.

  7. `qz test` Runner
     - Add `@test` attribute recognition in the parser/semantic.
     - `qz test` CLI command: discovers all functions marked `@test` in the project, compiles them into a single test harness binary, runs each, reports pass/fail.
     - A test fails if it panics (calls `panic()`). Passes if it returns normally.
     - Output format: `test module::name ... ok/failed` with a summary count.
     - Update CLI wiring in `src/main.rs` and `src/cli.rs`.

  8. [x] `pub` Visibility Enforcement on Types
     - Current: `pub struct`, `pub enum`, `pub trait`, `pub type` are parsed but `declare.rs` hardcodes `public: false` for all of them. Only functions enforce `public` during import resolution (S04 error).
     - Fix: in `src/semantic/declare.rs`, set `public: *pub_fn` (or equivalent parsed flag) for Struct, Enum, Trait, and TypeAlias symbol registration. Then in `declare_import_binding` (or type resolution), en
  force that importing a non-public type across modules emits S04 just like functions do.
     - Ensure `~/.quazi/std/src/` modules still compile — some types currently relied on as implicitly public may need `pub` added.

  ## P2 — Medium (only if P0/P1 done)

  9. [DONE] Threshold-Based Auto-Inline
     - Current: only `@inline` attribute marks functions for inlining.
     - Add heuristic auto-inline: functions with < 20 instructions and no recursion are auto-marked as inline candidates in `run_inline_candidate_pass` (`src/semantic/optimize.rs`).

  10. Cross-Basic-Block Const Folding
      - Extend const propagation beyond single expressions in `src/bytecode/codegen.rs`.



## FFI Roadmap — Full Plan

### What's done (Phase 1)
- `@api` / `@export` / `@repr(C)` / `@opaque` attributes
- Scalar + pointer types in FFI signatures (up to 6 SysV register args)
- `std.ffi`: `c_int`, `c_char`, etc.; `CStr`, `CString`, `nullptr[T]()`
- C source compilation via `[cc]` in `quazi.toml`
- Linking `.o`, `.a`, `.so` via `[link]` in `quazi.toml`
- `CallExt` in encoder: SysV / Win64 register args (scalars, pointers)

### Phase 2 — C Variadics (`printf`, `scanf`, etc.) (✅ DONE)
**Goal**: Allow calling C functions that use `...` (C variadics), e.g. `printf`, `dprintf`, `ioctl`.

Items:
1. [x] **Parser/AST**: add `c_variadic: bool` flag on `Fn` items and parameters.
   - Surface syntax: `@api("printf") unsafe fn printf(fmt: *c_char, ...);`
   - The `...` at the end means C variadic, not Quazi variadic. Parse as a bodyless decl only.
2. [x] **Semantic (`typecheck.rs`)**: lift the S14 "variadics not supported" error for `@api` functions when the variadic param uses `...` with no type annotation (C-style). Regular Quazi variadics still require a type.
3. [x] **Codegen (`codegen.rs`)**: `compile_api_fn` — set `flags = arg_count | C_VARIADIC_FLAG` on the `CallExt` instruction so the encoder knows to emit the ABI-required AL=0 for SysV float arg count.
4. [x] **Encoder (`encoder.rs`)**: in `CallExt`, if `C_VARIADIC_FLAG` is set, zero `rax` before the call (SysV requires `al` = number of XMM args for variadic calls).
5. [x] **`std.ffi`**: add `va_list` opaque struct and `c_variadic_fn` type alias pattern (documentation only; Quazi will not marshal `va_list` internally).
6. [x] **Test**: `examples/19-cvariadics/` — call `printf` and `dprintf` directly; verify output.

### Phase 3 — SysV Aggregate Arguments / Returns (struct by value)
**Goal**: Pass and return `@repr(C)` structs by value according to the SysV AMD64 ABI classification rules.

Items:
1. [ ] **ABI Classifier**: new module `src/backend/x86_64/sysv_abi.rs`.
   - `classify_type(fields: &[(TypeKind)], aliases) -> ArgClass`
   - `ArgClass`: `Integer`, `Sse`, `Memory` (for structs > 16 bytes or with unclassifiable fields).
   - `classify_struct(size, fields) -> (lo_class, hi_class)` — splits 8-byte halves.
2. [ ] **Codegen**: when a param/return type is a `@repr(C)` named struct:
   - If `Memory` class: pass pointer, emit `Lea` of the struct allocation, caller allocates stack slot.
   - If `Integer`/`Sse` in 1 or 2 eightbytes: load into register pairs (rdi+rsi, etc.).
3. [ ] **Encoder**: extend `CallExt` to accept struct register pairs via additional `CallArg` slots; emit `xmm` loads for `Sse`-class halves.
4. [ ] **Semantic**: lift the S14 "pass C structs through raw pointers" restriction for `@repr(C)` structs that the classifier accepts.
5. [ ] **Return aggregates**: for functions returning structs by value:
   - ≤ 16 bytes + `Integer` class: returned in `rax:rdx`.
   - `Memory` class: caller passes hidden first arg pointer (sret).
6. [ ] **Test**: `examples/20-repr-c-structs/` — pass and return `Point { x: f64, y: f64 }` by value from C.

### Phase 4 — SSE / Float Arguments
**Goal**: Pass `f32`/`f64` args in `xmm0`–`xmm7` per SysV ABI instead of integer registers.

Items:
1. [ ] **Encoder**: in `CallExt`, before calling, check each param type annotation stored in the chunk constants (needs new metadata in `CallExt` encoding or a separate `CallArgF` opcode).
2. [ ] **Option A (simpler)**: add a new opcode `CallArgF` that marks the next arg as floating-point; encoder moves it to the next `xmmN` register instead of the next `rdiN`.
3. [ ] **Codegen**: when compiling a call to an `@api` function, emit `CallArgF` for each `f32`/`f64` param.
4. [ ] **`std.ffi`**: expose `c_float` and `c_double` as aliases for `f32` and `f64` (already done); update docs noting SSE calling convention.
5. [ ] **Test**: `examples/21-ffi-floats/` — call `sin(x: f64)`, `fabs(x: f64)` from `libm`.

### Phase 5 — Callbacks / Function Pointers over FFI
**Goal**: Pass Quazi function pointers to C APIs (e.g. `qsort`, `pthread_create`).

Items:
1. [ ] **Semantic**: validate that a `fn(...)` type passed to an `@api` call is `@repr(C)`-compatible (no closures with captures, no generics, only FFI-safe param/return types).
2. [ ] **Codegen**: `FnAddr` const-pool entry already exists; ensure it generates a correct PLT relocation for cross-module function addresses.
3. [ ] **`@export` on lambdas**: allow `@export` on named top-level functions so they can be passed as C callbacks; already partially supported.
4. [ ] **`@no_mangle` + `unsafe fn`**: ensure calling convention of exported callback matches what C expects (SysV / Win64 depending on target).
5. [ ] **Test**: `examples/22-callbacks/` — pass a Quazi function to `qsort`; sort an integer array.

### Phase 6 — Foreign Global Variables
**Goal**: Read and write C global variables (e.g. `errno`, `stdout`, `environ`).

Items:
1. [ ] **Parser**: add `@api_global("symbol")` attribute for variable declarations.
   - Surface: `@api_global("errno") var errno: c_int;` at top level.
2. [ ] **Semantic**: validate `@api_global` — must be a top-level `var`, non-generic, FFI-safe type.
3. [ ] **Codegen**: emit a `Lea`-style reference: load address of the external symbol into a register; reads become `Load`; writes become `Store`.
4. [ ] **Sections/Relocations**: add the symbol as an undefined extern in the object file; linker resolves it.
5. [ ] **`std.ffi`**: expose `errno` as a getter function (preferred over direct global — avoids threading issues with `errno` macros) using `__errno_location()` on Linux.
6. [ ] **Test**: `examples/23-ffi-globals/` — read `stdout` FILE pointer; check `errno` after a failing syscall.

### Phase 7 — `qz header` Generator
**Goal**: Emit a C header (`.h`) for all `@export`-annotated Quazi functions, so C code can call into Quazi libraries.

Items:
1. [ ] **CLI**: `qz header <file|project>` subcommand.
2. [ ] **Type Printer**: map Quazi types → C type strings:
   - `i32` → `int32_t`, `u8` → `uint8_t`, `*u8` → `uint8_t*`, `void` → `void`, `@repr(C)` struct → struct forward decl, etc.
3. [ ] **Generator**: iterate `SemanticReport.exported_symbols`, render each as a C function prototype with `extern "C"` guards.
4. [ ] **Output**: write to `<stem>.h` next to the object or to `-o <path>`.
5. [ ] **Test**: compile a Quazi library to `.so`, generate its header, and include it from a C program that calls into Quazi.

---

### FFI Phase Priority Order
| Phase | Feature | Priority |
|-------|---------|----------|
| 2 | C Variadics (`printf`) | **DONE** |
| 3 | Aggregate args/returns | **HIGH** — needed for most real C APIs |
| 4 | SSE float arguments | **HIGH** — `sin`, `cos`, etc. |
| 5 | Callbacks/fn pointers | Medium |
| 6 | Foreign globals | Medium |
| 7 | Header generator | Low |

  ## Constraints
  - Do not create excess intrinsics or attributes.
  - Do not hardcode behavior that can be implemented in Quazilang itself.
  - Keep code clean and maintainable.
  - Update AGENTS.md to reflect any command changes or new behavior.
  - Run `cargo test` after each major change to ensure nothing breaks.

