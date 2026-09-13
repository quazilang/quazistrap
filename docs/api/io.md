# `std.io`

Audience: Quazi application developers.

`std.io` provides UTF-8 console text I/O. It owns strings returned from input
operations and validates bytes before creating `String`; raw descriptor I/O is
kept in `std.core`.

## Output

`stdout()` and `stderr()` return `Stdout` and `Stderr` writers implementing
`Write`. Their `write(text)` and `writeln(text)` methods write text, with the
latter appending `\n`. `writef` and `writelnf` are compiler-format-aware
variants; the final `any` argument convention is erased at the call site and
does not create a runtime dynamic value.

`print`, `println`, `err`, and `errln` are matching standard-output and
standard-error helpers. `writeln(fd, text)` writes to an arbitrary native file
descriptor. These APIs currently return `void`, so write failures and partial
writes are not observable through the public surface.

On Windows, stdout/stderr text uses UTF-16 console output when attached to a
real console and preserves UTF-8 bytes for redirected handles. The module does
not change the process console code page.

## Input

`read(delimiter)` reads stdin until the non-empty UTF-8 delimiter or EOF and
returns an owned `String` without the delimiter. `readln()` is
`read("\n")`; it normalizes Windows CRLF/CR input to `\n`. Custom delimiters
use immediate terminal mode where available and restore the original mode on
all normal error and success paths. Redirected input remains buffered.

`readkey()` reads one complete UTF-8 scalar, returning an owned empty string at
EOF. It does not require Enter in an interactive terminal. Neither operation
is a nonblocking API or offers cancellation/timeouts.

`ReadError` distinguishes allocation failure, capacity overflow, native input
failure, an empty delimiter, and invalid UTF-8. Invalid bytes are never exposed
as `String`.

## Limits and portability

Input grows an owned buffer as needed; there is no caller-configurable maximum
input length. Applications handling untrusted or unbounded streams should not
use these console helpers as a protocol reader. Console mode behavior is
implemented for Linux and Windows; behavior on other targets is not a supported
contract.
