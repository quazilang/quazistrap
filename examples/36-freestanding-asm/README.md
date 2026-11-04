# Freestanding Quazi + assembly (x86-64)

This is a **toolchain integration example**, not a bootable OS or a complete
bootloader. A loader must enter `_start` in 64-bit ring-0 mode with the image
mapped at the addresses described by `linker.ld`.

From this directory:

```sh
qz build --target x86_64-none --no-incremental
file build/freestanding-asm
readelf -h -l -s build/freestanding-asm
```

The Quazi project manifest automatically compiles `src/entry.S` using the C
compiler driver (`cc -x assembler-with-cpp -c`), generates the Quazi ELF
object, and links both with GNU ld or ld.lld using `linker.ld`. No Makefile,
libc, OS startup, or automatic Quazi runtime is involved. `_start` owns the
stack and calls the unmangled Quazi function `main`. It stores `42` in
`quazi_result` and halts; you can inspect this under a debugger after loading.

Use `[asm].assembler = "clang"` or `ASM_CC=clang` to choose a different
assembler driver; `[asm].flags` and `[asm].include-paths` control assembler
options. Plain `.s` files skip the C preprocessor. For one-off builds you can
use `qz build src/main.qz src/entry.S --target x86_64-none --linker-script
linker.ld -o build/image.elf`.

Actual booting requires adapting the linker layout and entry assembly to
UEFI, Multiboot, Limine, or another loader. Quazi does not supply a loader,
allocator, scheduler, interrupts, or firmware interfaces with this target.
