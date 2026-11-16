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

## Follow-up: one unified program (format 16)

The first fix wrote a format 15 container with one complete compilation per
operating system, so a single project became three copies of the same
program. Format 16 merges the per-target compilations into one module in
which each function body appears once with its target mask: for quazifetch,
109 bodies are shared by all three systems, 28 by two, and 40-44 are
specific to one; the file is 139 KB instead of 258 KB. The format 15
container and the short-lived `qz pack` command are removed (2026-11-16,
maintainer decision): projects are combined through dependencies, which
are linked into the one QZI. Limitation: each target is still analyzed and compiled once
(the merge happens on the compiled bodies), which costs build time but not
artifact size.
