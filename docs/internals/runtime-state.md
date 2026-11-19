# Process runtime state block

Quazi has no global variables. Process-wide runtime data lives in one
zero-initialized, 16-byte aligned, 128-byte data block, `__quazi_runtime_state`,
whose address `quazi.runtime.state()` returns. Owners of each range:

| Offset | Size | Owner | Meaning |
| --- | --- | --- | --- |
| 0–71 | 72 | prelude `qoroutine.qz` | scheduler queue lock/head/tail, init state, stack arena, free slots, preemption flag, outside panic count |
| 72 | 8 | prelude `qoroutine.qz` | isolated-thread registry spin lock (0 free, 1 held) |
| 80 | 8 | prelude `qoroutine.qz` | isolated-thread registry head: innermost `__qo_isolate` record first, found by `quazi.thread.current` |
| 88 | 4 | Windows crash report | thread id of the reporting thread (`lock cmpxchg` from 0); its own second fault exits 101, other faulting threads wait |
| 92–103 | 12 | — | reserved |
| 104 | 8 | std `os.qz` | environment writer spin lock (0 free, 1 held) |
| 112 | 8 | runtime / std `os.qz` | Linux `envp`: pointer to a null-terminated array of `NAME=VALUE` C strings |
| 120 | 8 | — | reserved |

`__quazi_envp` is a symbol at offset 112. The Linux `_start` stores the
kernel-provided `envp` there; `quazi.getenv` and the process-spawn runtime read
it on every call. `std.os.set_env`/`remove_env` build a new array under the
writer lock and publish it with one aligned 8-byte store, so a concurrent
reader sees either the old or the new complete array. Replaced arrays and
their strings are never freed (a reader may still hold them); each mutation
therefore keeps one copy of the environment alive.

Other targets do not use the slot: Windows and NetBSD keep the environment in
their system libraries.
