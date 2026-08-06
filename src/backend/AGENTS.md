# Backend (`src/backend/`)

```
src/backend/
├── mod.rs         # Backend trait, select_backend()
├── target.rs      # TargetSpec { arch, os, abi, emit_start }
├── linker.rs      # find + exec external linker
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

## Stack Frame

- **QZI reg N** → `[rbp-(N+1)*8]`.
- Frame size: `round_to_16(regs*8)` SysV, `round_to_16(regs*8+48)` Win64.

## Relocations

- `Plt32` (calls), `Pc32` (RIP-relative data).
- Addend always `-4`.

## Encoder Strategy

Emits dummy `call fn_start` / `lea rax,[fn_start]`, records pending relocs, zeros displacement bytes after assembly.

## Target Support

| OS | Format | ABI | Status |
|----|--------|-----|--------|
| Linux x86-64 | ELF64 | SysV | Full |
| Windows x86-64 | PE/COFF | Win64 | Full (needs `lld-link` + `LIB`) |
| macOS x86-64 | ~~Mach-O~~ ELF | SysV | Broken — `select_backend()` maps macOS to `ElfBackend`, `emit_start: false`, no Mach-O relocations |

## Native FFI and libraries

- Exported functions use `SymbolScope::Dynamic`; ordinary Quazi functions use
  linkage scope and intrinsic/API wrapper chunks remain compilation-local.
- `qz build source.qz native.o -L dir -l name` forwards native inputs to the
  linker. Project `[cc]` sources are compiled through `$CC` or `cc` with `-fPIC`.
- Windows omits `-fPIC`, includes the compiler-support archive needed for
  floating-point C objects, and `qz run` uses the same `[cc]`/`[link]` inputs as
  `qz build`.
- `--static-lib` emits an archive through `$AR` or `ar`; `--shared-lib` emits a
  Linux `.so` through the selected linker without a process start stub.
- Shared-library output is currently Linux x86-64 only. The compiler intentionally
  rejects unsupported FFI types before backend lowering.
