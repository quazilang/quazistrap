# D-029: maintainer answers of 2026-11-26

Status: decided by the maintainers on 2026-11-26 (asks.txt В31-В35).

- **В31А.** A socket has a single owner (D-014/D-019): every I/O operation
  needs exclusive access; to use a socket from a qoroutine the owner moves
  into it (`qo serve(stream)`) and data flows through `Channel[T]`. No
  descriptor clones or read/write halves. (The maintainer answered "maybe,
  whichever is better"; A was chosen because it matches D-014 and a later
  `split()` would not break the API.) Policy text: `docs/NETWORK.md`.
- **В32А.** TLS on Windows uses Schannel with the system certificate store;
  verified under Wine 11 (`scripts/check_tls.py --wine`, std `2ddd6be`).
- **В33А.** The implemented part of В29А for OS threads stays: a panic on a
  `thread.spawn` thread is returned by `join()` as
  `Err(ThreadError.Panicked)` without unwinding, an unjoined `JoinHandle`
  waits for its thread when destroyed, cancellation is cooperative
  (`handle.cancel()`, `thread.cancel_requested()`). Moving owned values into
  threads and channels between threads still waits for D-014.
- **В34А.** Both trait-object extensions, in this order: `&Concrete` passes
  as `&dyn Trait` (a temporary trait-object block for the loan), and a value
  of a generic `T: Trait` converts to `dyn Trait` (a vtable per
  specialization). Implemented in quazistrap `c223fd6`.
- **В35А.** The std public API follows one convention per concern (end of
  input is `Err(EndOfInput)`, `message()` returns `str`, paths are `Path`,
  both buffered writers flush on drop, `HashMap.get`, `_with_limits` for
  every decoder, `OpenFailed` everywhere, an empty CA path is
  `InvalidConfig`), with a [migration note](../migrations/std-api-unification.md).
  Implemented in std `83726e3` and `3573e3f` (the BufWriter part needed
  [D-028](drop-hook-fields.md), answer В36А).
