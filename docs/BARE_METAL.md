# Freestanding x86-64 ELF + native assembly

Quazi supports a freestanding target, `x86_64-none`, intended for kernels,
bootloaders, hypervisors and firmware components that supply their own entry
point and runtime. It is **not** a new instruction set, an OS, or a firmware
boot protocol. The compiler generates ordinary x86-64 ELF relocatable objects
using the SysV function ABI, then calls an external linker to lay out the image.

## Minimal project

```toml
[package]
name = "my-firmware"
version = "0.1.0"
std = false
crash_handler = false
mangling = false

[build]
entry = "src/main.qz"

[asm]
sources = ["src/entry.S", "src/interrupts.s"]
# assembler = "clang"     # optional CC-compatible assembler driver
# include-paths = ["src/include"]
# defines = ["CONFIG_SERIAL=1"]
# flags = ["-m64"]

[target.x86_64-none.link]
linker = "auto"            # system ld.lld or GNU ld; custom path also works
script = "linker.ld"       # GNU ld/ld.lld linker script, relative to project
libc = false
```

Use `qz build --target x86_64-none --no-incremental` from the project. Generated
native objects are placed in the project's `build/ffi/` directory, and the
final image in `build/`. `.S` files are assembled with the C preprocessor via
`cc -x assembler-with-cpp -c`; `.s` files are assembled without CPP via
`cc -x assembler -c`. Set `ASM_CC`, `CC`, or `[asm].assembler` to select a
compiler driver; it must support those GCC/Clang arguments. Each source can
call exported Quazi functions and vice versa through normal ELF symbols and
the x86-64 SysV ABI. The compiler does not interpret instructions in `.S`.

The `[link].script` key is a **linker script**, not an entire linker written in
Quazi. If you do write a custom linker executable, pass it using
`[link].linker = "/path/to/my-linker"` or `--linker`; it must implement the
GNU ld-style command-line arguments used by the toolchain (or provide a wrapper).
Target-specific `[target.x86_64-none.link]` overrides can set `script`,
`linker`, libraries, objects, flags and other existing link options.

For quick experiments outside a project:

```sh
qz build src/main.qz src/entry.S --target x86_64-none \
  --linker-script linker.ld -o image.elf
```

Other native inputs (`.o`, `.a`) continue to work. For raw object generation,
`qz build -c --target x86_64-none` emits a relocatable ELF object without
startup code. You can then use your own linker invocation separately.

## Freestanding constraints and responsibilities

- No Linux, Windows or NetBSD process startup and **no implicit libc** or
  dynamic interpreter. Quazi does not create `_start` for `x86_64-none`.
- `std = false` is required in a freestanding project: ordinary hosted `std`
  imports expect operating system services. Supply your own memory allocator,
  panic behavior and hardware I/O routines as needed, in Quazi or assembly.
- Quazi's normal SysV calling convention applies across the assembly boundary;
  the entry stub must initialize a valid aligned stack and CPU execution mode.
  Hardware interrupt handlers need their own ABI-correct assembly veneers.
- A linker script controls virtual/physical layout, sections, entry symbol,
  address alignment, and relocation policy. The `x86_64-none` path currently
  requires **GNU ld or ld.lld** for the final image; Quazi's hosted ELF linker
  does not interpret custom scripts.
- The output ELF does not boot on its own. Adapt your assembly and script to a
  bootloader, UEFI loader, or firmware environment. Linker layout choices are
  specific to your loader contract, not mandated by Quazi.
- Do not assume libc, TLS, OS threads, system calls, userspace signal handlers,
  or Linux `mmap` are available. Unsupported runtime imports correctly fail
  at link time instead of silently resolving to Linux implementations.

See `examples/36-freestanding-asm/` for a minimal standalone linkable ELF with
Quazi `main`, an assembly `_start` and a custom layout. That example is not an
OS kernel or a complete boot protocol implementation.
