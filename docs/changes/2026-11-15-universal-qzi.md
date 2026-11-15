# Universal QZI: every operating system in one artifact

Date: 2026-11-15

Audience: Quazi users and compiler developers.

`semantic::strip_cfg_for` resolved `@cfg(target_os = ...)` for the host
before QZI generation, so a QZI built on Linux held only Linux code: compiling
it for NetBSD failed ("refusing Linux syscalls"), and a NetBSD-built QZI
compiled for Linux failed on unresolved NetBSD libc symbols. A Linux QZI
compiled for Windows kept the Linux branch.

`qz build -i` now builds the program for Linux, Windows and NetBSD inside one
run (each with its own `@cfg` resolution and full analysis) and writes one
universal QZI (format 15) holding the three variants. `qz build app.qzi
--target T` selects T's variant; QZI dependencies are selected the same way.
A variant the program cannot compile for is omitted with a warning, and a
native build for it fails explicitly.

Build failures inside a variant no longer exit the process directly
(`fail_build`), so one system's errors do not abort the others.

Verification (`scripts/check_universal_qzi.py`, quazifetch by hand): one
quazifetch QZI built on Linux compiles for all three systems; the Linux
binary runs, the Windows binary runs under Wine, and the NetBSD binary is
byte-identical to a direct NetBSD build, contains no `syscall` instruction
and needs only `libc.so.12`. Not verified: running on a real NetBSD or
Windows machine, and QZI built on NetBSD or Windows hosts (the format and
selection do not depend on the host).
