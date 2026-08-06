# x86_64 Backend (`src/backend/x86_64/`)

## Entry Stubs (no CRT needed)

### Linux `_start`

`xor rbp,rbp` → zero sigaction struct → register handlers for `SIGSEGV`, `SIGABRT`, `SIGFPE`, `SIGBUS` → `call main` → `mov rdi,rax; mov rax,60; syscall`

### Windows `mainCRTStartup`

`sub rsp,40` → `AddVectoredExceptionHandler` → `call main` → `ExitProcess`

Both Linux and Windows stubs are generated via `iced-x86` `CodeAssembler` — raw-byte stubs with hand-computed relocations were removed to eliminate relocation-offset bugs.

## Portable C ABI lowering

- `ForeignSymbol` QZI constants describe source widths and aggregate fields;
  the encoder performs target-specific classification.
- SysV supports GP/SSE bank allocation, stack fallback, variadic `AL`, register
  aggregates through 16 bytes, and memory-class hidden sret.
- Win64 supports positional GP/XMM slots, variadic float duplication, aligned
  caller temporaries for indirect aggregates, direct 1/2/4/8-byte aggregates,
  shadow space, and hidden sret.
- C `f32` values convert to/from the internal f64-bit slot representation at
  argument, return, and `@repr(C)` field boundaries.

---

## Crash Handler

Identical output format on Linux & Windows:

- Prints `== CRASHED ==\n`
- Then `fatal: signal 0x<hex>` / `fatal: exception 0x<hex>`, `fault: 0x<addr>`, `rip: 0x<rip>`
- Checks `__quazi_trace_enabled`; if set, calls `__quazi_print_backtrace` (RBP chain walk, up to 16 frames)
- If not set, prints `use QUAZI_TRACE=1 to see full stack trace\n`

---

## Panic Handler

`PanicInfo` carries `message`, `file`, `line`. `__quazi_panic_handler` prints all three, then calls `__quazi_print_backtrace()` (intrinsic ID 25) before exiting 101.

`panic("msg")` call sites get `file`/`line` injected by codegen. `prelude/array.qz` uses `panic("index out of bounds")` for `Array.get` / `Array.set` / `Index` OOB checks.

---

## `@no_crash`

File-level directive (like `@no_std`). When present, the entry stub omits crash-handler registration (Linux `sigaction`, Windows `AddVectoredExceptionHandler`). `__quazi_print_backtrace` is still emitted so panic backtraces work. Produces a smaller binary (~1 KB smaller).

---

## `QUAZI_TRACE=1` Detection

- Windows `mainCRTStartup` uses `GetEnvironmentVariableA`.
- Linux `_start` calls `getenv` (libc is already linked, the dynamic linker initialises `environ` before entry).
- Both set the `__quazi_trace_enabled` byte in `.data`.

---

## Encoder Silent Fallback (Fixed)

Previously at `encoder.rs:1607–1612`, unimplemented opcodes (`NewObj`, `Move`, `Drop`, `Dup`, `Spawn`, `AtomicAdd`, `AtomicCas`, `StrConcat`) emitted `xor rax,rax; mov slot(0), rax` — producing wrong code instead of crashing.

Now they `panic!("encoder: unimplemented opcode {:?}", ...)` instead.
