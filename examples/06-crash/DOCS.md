# Example: 06-crash

Demonstrates the crash handler by dereferencing a null pointer.

- Uses `unsafe` block
- Sets a raw pointer to `0` and dereferences it
- Useful for testing the `__quazi_crash_handler` / `__quazi_print_backtrace` output when `QUAZI_TRACE=1`
