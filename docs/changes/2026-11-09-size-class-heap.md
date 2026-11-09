# Size-class heap in the embedded Linux runtime

Audience: language users and compiler developers.

Every `malloc` in a Linux program without libc was its own `mmap` (a whole
page and a system call) and every `free` a `munmap`. A loop building 300000
strings and structs made 4.2 million system calls and took 8.2 s.

The embedded runtime (`backend/x86_64/runtime.rs`) now keeps seven size
classes (32 to 2048 bytes including the 16-byte header) on free lists behind a
spin lock in a new `__quazi_heap_state` data block, refilled 64 KiB at a time.
Larger blocks keep their own mapping. `calloc` and `realloc` call `malloc` and
`free`. Runtime routines gained relocations (`RuntimeRelocation`) so they can
address the heap state and call each other.

The same loop now takes 0.19 s with almost no system calls; four threads
allocating concurrently produce exact results. A qoroutine preempted while it
holds the heap lock delays other allocators only until its worker's next
preemption tick lets the queue run it again.

Found while testing: integer-to-string conversion writes into one static
buffer per call site, so threads converting numbers in the same code race
(pre-existing; tracked separately).

Verification: `scripts/check_heap.py` (fewer than 1000 mmap/munmap calls for
the 300000-round loop under strace, exact results, four concurrent threads
5 runs; fails with the previous compiler); all Rust tests, e2e scripts
(threads, qoroutines, preemption, JIT), std suite and examples.
