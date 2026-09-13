# Linux TCP sends no longer terminate on a closed peer

Audience: Quazi network users and standard-library maintainers.

Linux `TcpStream.send`, `send_bytes`, and `send_raw` now use the native socket
send operation with `MSG_NOSIGNAL`. A closed peer returns the existing
`NetError.BrokenPipe` result instead of allowing `SIGPIPE` to terminate the
process. The public API and Windows behavior are unchanged.

Verification uses a local Unix socket pair: closing the peer and sending on
the remaining `TcpStream` must keep the program alive and yield `BrokenPipe`.
The standard network module also compiles for Windows x86-64 COFF.
