Implement the following changes to the Quazilang compiler (C:\Users\nam\desktop\codes\void), in exact priority order. Do NOT touch LSP — it is lowest priority. Update DOCS.md as you go.

  ## P0 — Critical Safety / Bugs

  1. Enhanced Crash/Panic Handler
     - Current: Linux `__quazi_crash_handler` and Windows `__quazi_crash_handler_win` print a static 78-byte message with no context.
     - Goal: include (a) the panic message string if available, (b) file/line info from the panic site, (c) optional stack trace via rbp frame walking when `QUAZI_TRACE=1` is set.
     - Files: `src/backend/x86_64/start.rs`, `src/backend/x86_64/sections.rs`, `std/src/panic.qz`, `std/src/core.qz`.
     - Keep binary size minimal; use compile-time-known offsets where possible.

  2. Fix Encoder Silent Fallback
     - Current: in `src/backend/x86_64/encoder.rs` around line 1607, unimplemented opcodes (`NewObj`, `Move`, `Drop`, `Dup`, `Spawn`, `AtomicAdd`, `AtomicCas`, `StrConcat`) silently emit `xor rax,rax; mov slot
  (0),rax` — producing wrong code.
     - Fix: replace silent fallback with `panic!("unimplemented opcode in encoder: {:?}", op)` or return a proper `Err`. Wrong code is worse than a crash.

  3. Fix Non-Slice Iterator Codegen
     - Current: in `src/bytecode/codegen.rs` around line 1624, `ForLoop::Each` on non-slice collections emits a broken infinite loop (`Jz` + `Jmp` to top with no `.next()` call).
     - Fix: since `Iterator[T]` trait exists (`std/src/prelude/traits.qz`) with `next()` and `has_next()`, emit proper iterator protocol calls: bind iterator result, loop while `has_next()`, call `next()` to
  get value. Do not change the AST; fix only the codegen path.

  ## P1 — High Impact

  4. Bitwise Operators (`&` `|` `^` `<<` `>>`)
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

  5. AOT `@cfg` Stripping
     - Current: `@cfg` conditions are evaluated in semantic passes (declare, typecheck, unused) but dead `CfgBlock` AST nodes are still passed to codegen.
     - Fix: add a pre-codegen AST pruning pass that removes statement-level `CfgBlock` nodes whose condition is false. This should happen after semantic analysis but before `compile_program`. Also strip `@cfg`
  -disabled top-level items from the program item list so codegen never sees them.

  6. `void link` Built-in Linker
     - Goal: when `void build myprog.o` is invoked (an object file as input), use a built-in minimal linker to produce the final binary instead of calling the system linker.
     - For ELF (Linux): parse the existing `.o` (or produce it internally), emit a single ELF executable with `.text`, `.rodata`, `.data`, `.bss`, entry point `_start`, and `PT_LOAD` segments. Target <500B for
  a hello-world binary.
     - For PE-COFF (Windows): emit a minimal PE with `.text`, `.rdata`, `.data`, entry `mainCRTStartup`, and correct headers. Target <700B.
     - Reuse existing `src/backend/x86_64/start.rs` stubs. Reuse existing section/symbol/relocation logic where possible.
     - The built-in linker should be triggered automatically when the input to `void build` ends in `.o` and the user did not pass `--obj`. It should also be usable as `void build --link myprog.o` if that is c
  leaner for the CLI.
     - Update `src/cli.rs` and `src/main.rs` command routing as needed.

  7. `void test` Runner
     - Add `@test` attribute recognition in the parser/semantic.
     - `void test` CLI command: discovers all functions marked `@test` in the project, compiles them into a single test harness binary, runs each, reports pass/fail.
     - A test fails if it panics (calls `panic()`). Passes if it returns normally.
     - Output format: `test module::name ... ok/failed` with a summary count.
     - Update CLI wiring in `src/main.rs` and `src/cli.rs`.

  8. `pub` Visibility Enforcement on Types
     - Current: `pub struct`, `pub enum`, `pub trait`, `pub type` are parsed but `declare.rs` hardcodes `public: false` for all of them. Only functions enforce `public` during import resolution (S04 error).
     - Fix: in `src/semantic/declare.rs`, set `public: *pub_fn` (or equivalent parsed flag) for Struct, Enum, Trait, and TypeAlias symbol registration. Then in `declare_import_binding` (or type resolution), en
  force that importing a non-public type across modules emits S04 just like functions do.
     - Ensure `std/src/` modules still compile — some types currently relied on as implicitly public may need `pub` added.

  ## P2 — Medium (only if P0/P1 done)

  9. Threshold-Based Auto-Inline
     - Current: only `@inline` attribute marks functions for inlining.
     - Add heuristic auto-inline: functions with < 20 instructions and no recursion are auto-marked as inline candidates in `run_inline_candidate_pass` (`src/semantic/optimize.rs`).

  10. Cross-Basic-Block Const Folding
      - Extend const propagation beyond single expressions in `src/bytecode/codegen.rs`.

  ## Constraints
  - Do not create excess intrinsics or attributes.
  - Do not hardcode behavior that can be implemented in Void itself.
  - Keep code clean and maintainable.
  - Update DOCS.md to reflect any command changes or new behavior.
  - Run `cargo test` after each major change to ensure nothing breaks.