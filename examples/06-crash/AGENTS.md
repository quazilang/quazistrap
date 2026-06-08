# Example: 06-crash

Demonstrates the crash handler by dereferencing a null pointer.

- Uses `unsafe` block
- Sets a raw pointer to `0` and dereferences it
- Useful for testing the `__void_crash_handler` / `__void_print_backtrace` output when `VOID_TRACE=1`
