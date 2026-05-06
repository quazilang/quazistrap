# void

void - the programming language.

this programming language was designed to be fast, understandable, strict, and very flexible.

## what we have done

* [x] lexer & tokenizer
* [x] parser & AST
* [x] semantic analysis (scope, duplicate declarations)
* [x] type checking (done, not fully)
* [ ] codegen
* [ ] memory allocators
* [x] module system (void.toml + local deps)
* [ ] standard library
* [x] cli and build system (compile/build/run/check)
* [ ] project scaffolding (new/fmt/clean)
* [ ] finish bootstrapping
* [ ] rewrite in itself

## contributors

* namnam1105 - made lexer tokenizer and ast
* amapekibert - made spans, full generic syntax, readable diagnostics

## license

this project is licensed under the bsd zero clause license.
see the LICENSE file for details.

## project config (void.toml)

minimal example:

```toml
[package]
name = "hello"
version = "0.1.0"

[build]
entry = "src/main.void"   # optional, defaults to src/main.void
src = "src"               # optional, defaults to src
flags = ["-O2"]           # optional, passed to gcc when emitting binaries

[dependencies]
utils = { path = "../utils", version = "0.1.0" }
```

if a void.lock file exists, it is used to pin dependency versions. when missing and
dependencies are present, a lockfile is created on build/run.

## syscalls and platform apis

stdlib functions can bind to OS services via built-in attributes:

- linux/posix: `@syscall("write")` or `@syscall(1)` emits a raw syscall.
	args map to `rdi, rsi, rdx, r10, r8, r9` and the return value is in `rax` (mapped to `r0`).
- windows: `@api("WriteFile")` emits a direct Win32 API call.
	args map to `rcx, rdx, r8, r9`, remaining args go on the stack after a 32-byte shadow space;
	return value is in `rax` (mapped to `r0`).
