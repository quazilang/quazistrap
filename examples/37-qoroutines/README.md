# 37-qoroutines

Concurrency with `qo` qoroutines and channels ([D-019](../../docs/decisions/qoroutines.md)):

1. `qo f(args)` runs a call in a new qoroutine; `join()` waits and returns
   `Result[T, QoroutineError]`.
2. A producer and a consumer share a bounded `Channel[i64]`: `send` waits
   while it is full, `receive` returns `None` once it is closed and drained.
   `clone()` creates another owner of the same channel.
3. A panic inside a qoroutine prints its message and makes `join()` return an
   error; the program keeps running.

```sh
qz run
```

Qoroutines run on a pool of four worker threads; on Linux a qoroutine that
runs for 10 ms without waiting is preempted. String literals interpolate
values (`"sum is {received}"`).
