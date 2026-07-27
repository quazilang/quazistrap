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

- **SysV** (Linux/macOS): `rdi,rsi,rdx,rcx,r8,r9` → `rax`.
- **Win64**: `rcx,rdx,r8,r9` → `rax`; args 5-6 at `[rsp+32/40]`.

## Stack Frame

- **VBC reg N** → `[rbp-(N+1)*8]`.
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
