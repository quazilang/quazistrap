# Stateful child-process polling

Audience: Quazi application developers and standard-library maintainers.

`Child.try_wait()` now takes an exclusive receiver. A live child remains usable;
an exited child has its handle cleared before its status is returned. `wait()`
and `close()` remain consuming operations. This corrects the previous
unimplementable contract in which a runtime-conditional `try_wait` result was
said to consume an affine local.

Compatibility: ordinary calls on mutable `Child` locals remain source-compatible.
Code that called `try_wait()` on a temporary must retain it in a mutable local.

Verification: the standard-library process regression covers both possible
polling outcomes: a later `wait()` yields no second status after an exited poll,
or it yields the status after a live poll.
