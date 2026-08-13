# Backend (`src/backend/`)

```
src/backend/
├── mod.rs         # Backend trait, select_backend()
├── target.rs      # TargetSpec { arch, os, abi, emit_start }
├── builtin_linker.rs # static multi-object x86-64 ELF linker
├── linker.rs      # choose built-in or invoke an explicit external linker
└── x86_64/
    ├── encoder.rs # FnEncoder: Chunk → (bytes, PendingReloc[])
    ├── sections.rs / symbols.rs / relocations.rs / start.rs
```

## Calling Conventions

- **SysV** (Linux/macOS): independent integer and SSE register banks, stack
  overflow arguments, up-to-two-eightbyte aggregate classification, and hidden
  sret pointers for memory-class returns.
- **Win64**: four positional GP/XMM slots, 32-byte shadow space, stack overflow
  arguments, indirect non-1/2/4/8-byte aggregates, and hidden sret pointers.
- C ABI values are normalized at synthetic export adapters. Ordinary Quazi
  calls continue to use the internal eight-byte-slot ABI.
- `CallCReg` applies the same SysV/Win64 classification to an indirect raw
  function pointer. Exported functions become callback values through the
  address of their synthetic C adapter.
- Foreign globals use undefined data symbols and PC-relative relocations in
  ELF64 and PE/COFF. The frontend limits them to scalar/pointer-sized values;
  typed loads and stores preserve C widths and `f32` conversion.

## Stack Frame

- **QZI reg N** → `[rbp-(N+1)*8]`.
- Frame size: `round_to_16(regs*8)` SysV, `round_to_16(regs*8+48)` Win64.

## Relocations

- `Plt32` (calls), `Pc32` (RIP-relative data).
- Addend always `-4`.

## Encoder Strategy

Emits dummy `call fn_start` / `lea rax,[fn_start]`, records pending relocs, zeros displacement bytes after assembly.

## Target Support

Plain Linux executable builds use the built-in linker automatically. It links
compiler-produced and passed ELF objects into a static ELF with separate RX/R/RW load
segments, a non-executable stack, checked PC-relative/absolute relocations, and
no implicit native libraries. `--linker builtin` forces this path. Additional
ELF `.o` files remain on the built-in path; archives, shared libraries, `-l`,
`-L`, or an explicit external linker select the external path. libc/libm are
linked only when the user requests them with `-l`.

`qz build` and `qz run` accept ELF `.o` files alongside Quazi source/QZI input,
or as an object-only input list. When `_start` is absent, the linker synthesizes
a minimal 15-byte entry that calls `main` and exits with the Linux syscall. This
minimal path does not construct command-line arguments or install the full
compiler-generated crash handler.

The experimental linker rejects unsupported allocated sections, relocations,
native flags, duplicate strong symbols, and unresolved external symbols with
actionable errors. Non-loaded metadata/debug sections are omitted. Its current
targets are x86-64 Linux and Windows; Mach-O, archives, shared objects, TLS, COMDAT,
and linker scripts remain follow-up work. See `docs/LINKER.md` for the complete
input, output, runtime, selection, and limitation contracts.

| OS | Format | ABI | Status |
|----|--------|-----|--------|
| Linux x86-64 | ELF64 | SysV | Full |
| Windows x86-64 | PE/COFF | Win64 | Built-in PE32+ for supported COFF; external linker for native libraries |
| macOS x86-64 | ~~Mach-O~~ ELF | SysV | Broken — `select_backend()` maps macOS to `ElfBackend`, `emit_start: false`, no Mach-O relocations |

Windows argument-taking entry points parse the Unicode command line with
`GetCommandLineW`/`CommandLineToArgvW`, convert each argument to UTF-8, and keep
the Quazi `Array[str]` outside the Win64 caller shadow space. This requires
`shell32.lib` in addition to Kernel32. Windows links the system `ws2_32.lib`
so `std.net` works without project-specific linker configuration; CRT libraries
remain opt-in.

## Native FFI and libraries

- Exported functions use `SymbolScope::Dynamic`; ordinary Quazi functions use
  linkage scope and intrinsic/API wrapper chunks remain compilation-local.
- Export adapter code has two symbols at the same address: the stable dynamic
  `@export` name for C consumers and a compilation-local synthetic name used by
  Quazi callback-address relocations, including portable QZI inputs.
- `qz build source.qz native.o -L dir -l name` forwards native inputs to the
  linker. Project `[cc]` sources are compiled through `$CC` or `cc` with `-fPIC`.
- Windows omits `-fPIC`, includes the compiler-support archive needed for
  floating-point C objects, and `qz run` uses the same `[cc]`/`[link]` inputs as
  `qz build`.
- `--static-lib` emits an archive through `$AR` or `ar`; `--shared-lib` emits a
  Linux `.so` through the selected linker without a process start stub.
- Shared-library output is currently Linux x86-64 only. The compiler intentionally
  rejects unsupported FFI types before backend lowering.
