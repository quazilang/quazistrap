# Accurate cross-target native build labels

The native build-progress label now reports the selected `--target` rather
than the operating system running the compiler. For example, a Linux-hosted
`qz build --target x86_64-windows -c` now reports `x86_64.windows`, matching
its emitted COFF object.

This is a presentation-only correction: the compiler already selected the
requested backend and object format. A focused regression covers Linux and
Windows target labels.
