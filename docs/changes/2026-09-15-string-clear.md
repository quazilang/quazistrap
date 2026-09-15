# In-place String cleanup

`String.clear(self: &String!)` releases an owned string allocation and resets
the same owner to the canonical empty state. It is the reusable in-place
operation for code that must replace a string field or loop-local value.

`String.free(self: String)` remains the consuming destructor: a direct call
transfers the owner and it cannot be used again. This distinction lets INI
parsing release replaced content without relying on an unsupported partial move
or a shallow alias.

The source-backed INI library now uses shared receivers for queries, an
exclusive receiver for parsing/replacement, and consuming cleanup only at the
end of ownership. The file-roundtrip consumer keeps its `File` mutable for its
exclusive write operation.

Verification covers the INI library's four source tests and its consumer's
semantic check plus Linux object build.
