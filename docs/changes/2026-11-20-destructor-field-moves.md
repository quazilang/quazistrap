# Destructors track the fields they move out

Date: 2026-11-20

Audience: Quazi users who write `free(self: T)` hooks.

A consuming `free` hook may still dispose of each owned field of `self`
once (`self.content.free();`, `take(self.inner);`, `const f = self.file;`).
The compiler now remembers that move: using the field again, moving it a
second time or assigning it afterwards is S10 ("use of moved field"), as in
any other method. Before, these compiled and read or destroyed freed memory.

Code that freed a field and then assigned a fresh value to it, a pattern
once used to keep the struct "valid" after its hook ran, must drop the
assignment:

```text
pub fn free(self: Doc) void { self.content.free(); self.content = String.new(); }
                                            ->
pub fn free(self: Doc) void { self.content.free(); }
```

Assigning an owned field anywhere else now destroys the value it replaces
(the old value was leaked before).
