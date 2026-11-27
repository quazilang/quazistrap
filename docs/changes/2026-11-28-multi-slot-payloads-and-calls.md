# Multi-slot enum payloads, function values and raw pointers

Date: 2026-11-28

Audience: Quazi users; stage 2 of the fat `str` plan
([internals](../internals/fat-str.md)).

- Enum payloads may be multi-slot values: `Option[[i64; 2]]`,
  `Result[[i64; 2], E]`, user variants such as `Pair([i64; 2], i64)`.
  Payloads are stored inline after the discriminant; constructors,
  `match` bindings (also nested and consuming), `?` and drop glue use each
  payload's slot count. Before, these were S14.
- Calls through `fn` values passed and returned only one word per value:
  `const f: fn() [i64; 3] = make; f()` and `fn([i64; 3], i64)` values
  produced garbage (also through generic `call[T](f: fn() T)`). Function
  values now use the direct-call ABI (hidden result pointer, every slot of
  every argument); named functions used as values get a matching forwarder.
- Reading or writing through a raw pointer to a multi-slot type
  (`*(p as *T)` with `T = [i64; 3]`) moved one word. It now moves every slot,
  which `Channel[T]`, qoroutine results and `std.thread` results rely on;
  the qoroutine task record and `JoinHandle` block keep the result after
  their fixed fields, sized by `size_of[T]()`.

Regression: `scripts/check_fixed_arrays.py` (enums case, also under
`--valgrind`).
