# x86_64 Backend (`src/backend/x86_64/`)

## Entry Stubs (no CRT needed)

### Linux `_start`

`xor rbp,rbp` → zero sigaction struct → register handlers for `SIGSEGV`, `SIGABRT`, `SIGFPE`, `SIGBUS` → `call main` → `mov rdi,rax; mov rax,60; syscall`

### Windows `mainCRTStartup`

`sub rsp,40` → `AddVectoredExceptionHandler` → `call main` → `ExitProcess`

Both Linux and Windows stubs are generated via `iced-x86` `CodeAssembler` — raw-byte stubs with hand-computed relocations were removed to eliminate relocation-offset bugs.

## Portable C ABI lowering

Linux objects embed only the allocation/memory runtime routines they reference:
`malloc`, `calloc`, `realloc`, `free`, `memcpy`, `memmove`, `memset`, and
`memcmp`, `strcpy`, and `strcat`. Allocation uses direct `mmap`/`munmap`
syscalls with a private
16-byte size header; no libc allocator is required.

The `quazi.sleep_ms` intrinsic is also self-contained on Linux: it lowers to
the `nanosleep` syscall with a stack-local `timespec`, not an external
`usleep` call. Built-in-linked executables therefore require no libc merely to
use `std.os.sleep`.

On Windows, `quazi.sleep_ms` preserves its `u64` input by splitting delays
longer than the largest finite Win32 `Sleep` DWORD into finite chunks. Never
pass `0xffff_ffff` to `Sleep`: it is the `INFINITE` sentinel, not a duration.

- `ForeignSymbol` QZI constants describe source widths and aggregate fields;
  the encoder performs target-specific classification.
- SysV supports GP/SSE bank allocation, stack fallback, variadic `AL`, register
  aggregates through 16 bytes, and memory-class hidden sret.
- Win64 supports positional GP/XMM slots, variadic float duplication, aligned
  caller temporaries for indirect aggregates, direct 1/2/4/8-byte aggregates,
  shadow space, and hidden sret.
- C `f32` values convert to/from the internal f64-bit slot representation at
  argument, return, and `@repr(C)` field boundaries.
- `CallCReg` shares the direct `CallExt` argument/return lowering, but loads the
  callback address from its QZI slot and emits `call r11` instead of a symbol
  relocation. This keeps one portable signature for Linux SysV and Win64.
- `ForeignGlobal` constants lower to RIP-relative addresses with `Pc32` data
  relocations. Subsequent QZI `Load`/`Store` flags select byte/word/dword/qword,
  signed extension, and `f32` conversion identically on SysV and Win64.

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

## Package startup and symbol settings

`[package].crash_handler = false` omits crash-handler registration while keeping
the platform process entry. `[package].mangling = false` emits bare native
function names and rejects collisions. Both settings default to `true`; removed
source attributes must not be reintroduced.

---

## `QUAZI_TRACE=1` Detection

- Windows `mainCRTStartup` uses `GetEnvironmentVariableA`.
- Linux `_start` walks `envp` from the kernel-provided initial stack; it does
  not require libc or a dynamic loader.
- Both set the `__quazi_trace_enabled` byte in `.data`.

---

## Encoder Silent Fallback (Fixed)

Previously at `encoder.rs:1607–1612`, unimplemented opcodes (`NewObj`, `Move`, `Drop`, `Dup`, `Spawn`, `AtomicAdd`, `AtomicCas`, `StrConcat`) emitted `xor rax,rax; mov slot(0), rax` — producing wrong code instead of crashing.

Now unsupported opcodes and unknown intrinsic IDs return `BackendError` instead
of emitting plausible but incorrect machine code or panicking.

Win64 runtime intrinsic calls reserve the required 32-byte shadow space plus
aligned stack arguments. `ReadFile`/`WriteFile` store their byte counts in the
reserved scratch slot outside callee-owned shadow space and map API failure to
`-1`. Linux integer and floating-point formatting is emitted
inline and does not import `sprintf`; Windows keeps its target-specific native
formatting path.

## Dynamic vtables

Source and QZI-only emission consume retained `VtableLayout` constants. Validate
all target chunks and layout conflicts, then map exact Quazi method names to
the actual native function-symbol table. Hex-encoded identity components avoid
punctuation collisions; marker tables reserve one word. Table relocation errors
propagate as BackendError. Historical layout-free addresses require a source
rebuild. No ownership/effect proof follows from a retained table.

## Arithmetic checkpoint (2026-10-22)

QZC26 supersedes QZC25; QZI13 remains Unverified. `semantic/arithmetic.rs`
shares mutation, eager arithmetic and negation requirements over declaration
slots and every concrete identity. Only view Add and pointer-left byte offsets
are special cases. Auto-deref flags describe physical loads; shared string
wrappers preserve views and one exclusive wrapper loads once. Aliases normalize
before collapse. Floating Mod uses guarded FPREM reduction with balanced x87
stack; constprop agrees. General bitwise/Pow, coercion and effect proof remain
open. See [the contract](../../../docs/internals/arithmetic.md).

## Typed-power checkpoint (2026-10-23)

QZC28 / QZI14 implement the [typed-power contract](../../../docs/internals/power.md).
Numeric declaration-slot requirements and known constant checks are separate
from runtime checked integer squaring. Integer/integer power emits portable
loops, not Pow; floating Pow requires explicit operand representation flags.
Legacy Pow artifacts require source rebuilding. Floating lowering imports no
libm and retains integer exponent parity; fractional power is approximate.
Artifacts remain Unverified; general coercion, String ABI and effects stay open.

## ELF stack declaration (2026-10-23)

`SectionAccumulator::new` adds an empty `.note.GNU-stack` only for ELF, using
SectionKind::Other (SHT_PROGBITS) and explicit zero ELF flags. Do not use Note:
this marker is not an ELF note payload. Source/QZI/cached object production
shares this path, including startup/no-start and archive/shared output. COFF
stays marker-free. External linker options/native inputs retain their stack
policy; the builtin linker already emits RW PT_GNU_STACK. No cache/wire version
change is needed. Regression: `scripts/check_elf_stack.py`; see
[verification](../../../docs/changes/2026-10-23-elf-stack-metadata.md).
