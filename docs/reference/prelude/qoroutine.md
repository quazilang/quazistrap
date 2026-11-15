# `prelude.qoroutine`

prelude.qoroutine — built-in qoroutines (D-019).

`qo f(args)` is lowered by the parser to `__qo_spawn(|| f(args))`; the
result is a `Qoroutine[T]` whose `join()` waits and returns `T`.

M:N scheduler: qoroutines run on four OS worker threads started on first
use. Each qoroutine owns a 256 KiB stack slot and a context entered with
`quazi.context.switch`. Quazi has no global variables, so the scheduler
lives in the `quazi.runtime.state` block:
  +0 queue lock, +8 queue head, +16 queue tail,
  +24 init state (0 none, 1 initializing, 2 ready), +32 stack arena,
  +40 free slot indexes (i64 array), +48 free slot count,
  +56 preemption enabled (the signal handler was installed).
All stacks are slots of one arena; a slot's first word is its task, so code
running on a qoroutine finds its task from the stack pointer
(`quazi.stack.pointer` + `quazi.ptr.offset_from`, no thread-local storage).
Task block (512 bytes):
  +0 joinable (2 once finished), +8 result slot, +16 closure, +24 slot index,
  +32 next in queue, +40 worker context, +48 type-erased runner,
  +56 finished flag, +64 task context (272 bytes), +336 panicked flag,
  +344 preemption hold count.
A finished task is published as joinable by its worker only after the
worker is back on its own stack, so `join` may free the task's stack.

A panic inside a qoroutine prints its message, abandons the qoroutine's
stack (pending destructors there do not run) and makes `join` return
`Err(QoroutineError.Panicked)`; elsewhere a panic still ends the program.

Preemption (Linux): every worker arms a 10 ms thread timer that sends
SIGURG to it. The handler runs on the interrupted qoroutine's stack and, if
the qoroutine holds no runtime lock (+344 is zero), switches to the worker
from inside the signal frame; resuming the qoroutine returns from the
handler and `rt_sigreturn` restores every register. The worker unblocks
SIGURG again after each switch. A task that is not running always holds
at least one preemption hold (fresh tasks start with one, voluntary and
preempting switches take one), so a signal during the switch into a task
cannot re-enter it. Windows, NetBSD and programs linking libc (where a
preempted libc call could deadlock or migrate its thread state) are not
preempted.

Current limits: `join` outside a qoroutine yields its OS thread; at most
`__qo_slot_count()` qoroutines are alive at once. Owned results move into
the result slot once and out through `join`, which reads the slot only for
a qoroutine that finished without panicking.

## Contents

- [`QoroutineError`](#qoroutineerror) (enum)
- [`Qoroutine`](#qoroutine) (struct)
- [`Channel`](#channel) (struct)

## `QoroutineError`

```quazi
pub enum QoroutineError {
    // The qoroutine panicked; its message was printed to stderr.
    Panicked,
}
```

Why `join` could not return the qoroutine's result.

## `Qoroutine`

```quazi
pub struct Qoroutine[T] { /* internal fields */ }
```

A running or finished qoroutine started by `qo`.

### `Qoroutine.join`

```quazi
pub fn join(self: Qoroutine[T]) Result[T, QoroutineError]
```

Wait for the qoroutine to finish: its result, or an error if it panicked.

## `Channel`

```quazi
pub struct Channel[T] { /* internal fields */ }
```

A queue that passes values between qoroutines: `send` adds a value,
`receive` waits for one and returns `None` once the channel is closed and
empty. Clones share the same queue.

### `Channel.new`

```quazi
pub fn new(capacity: usize) Channel[T]
```

A channel buffering up to `capacity` values (at least one).

### `Channel.clone`

```quazi
pub fn clone(self: &Channel[T]) Channel[T]
```

Another owner of the same channel, e.g. to pass to a qoroutine.

### `Channel.send`

```quazi
pub fn send(self: &Channel[T], value: T) void
```

Append `value`, waiting while the channel is full. Panics once closed.

### `Channel.receive`

```quazi
pub fn receive(self: &Channel[T]) Option[T]
```

The oldest value, waiting while the channel is empty; None once it is
closed and drained.

### `Channel.close`

```quazi
pub fn close(self: &Channel[T]) void
```

No more values will be sent; receivers drain what is buffered.

### `Channel.free`

```quazi
pub fn free(self: Channel[T]) void
```

Releases this handle; the queue is freed with its last clone.
