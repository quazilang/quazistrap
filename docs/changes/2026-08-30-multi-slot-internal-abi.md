# Multi-Slot Internal ABI

Date: 2026-08-30

The compiler now carries `RuntimeValueLayout` records from semantic analysis
into bytecode generation. Fixed arrays can cross ordinary and generic function
boundaries by value as contiguous register blocks.

Results wider than one slot use a hidden sret pointer in the first call
argument. Callees write result slots from fixed registers `r1..rN`; callers
reserve a pinned register block and copy from the sret buffer. `ArrayLoad` and
`ArrayStore` use the encoded element-slot count for both stride and block copy,
so `Array[[i32; 3]]` and `Box[[i32; 3]]` preserve all elements.

The register allocator now treats multi-slot returns, storage writes, and
sret-backed loads as adjacency-sensitive operations. This prevents dead-code
elimination and register remapping from dropping or reordering value slots.

Generic calls must resolve to an emitted mangled specialization. Missing
specialization metadata is a code-generation error rather than a fallback to
the one-slot generic template.

Multi-slot variadic elements, inline enum payloads, and inline struct fields
remain rejected while their storage and ownership rules are completed.
