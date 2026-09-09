# Chapter 1: Hello, World!

Welcome to Quazi! This chapter gets you from zero to running your first
program.

## Your first program

Create a directory for your project and a source file:

```bash
mkdir hello && cd hello
qz init
```

This creates a `quazi.toml` manifest and a `src/main.qz` entry point. Open
`src/main.qz` and write:

```quazi
// tutorial: runnable
import std.io;

fn main() i32 {
    io.println("Hello, world!");
    ret 0;
}
```

## Running it

```bash
qz run
```

You should see:

```
Hello, world!
```

## What happened?

- `import std.io;` — loads the I/O module from the standard library.
- `fn main() i32` — declares the program entry point. `main` returns an
  `i32` exit code.
- `io.println("Hello, world!");` — prints text followed by a newline.
- `ret 0;` — returns exit code 0 (success) to the operating system.

## Building without running

```bash
qz build
```

This produces an executable in the `build/` directory. Run it directly:

```bash
./build/hello
```

## Checking without building

```bash
qz check
```

This runs the frontend (parsing and analysis) without producing an
executable. Useful for catching errors quickly during development.

## Command-line arguments

`main` can also accept command-line arguments:

```quazi
// tutorial: runnable
import std.io;

fn main(args: Array[str]) i32 {
    io.println("Program: {}", args[0]);
    io.println("Arguments: {}", args.len() - 1);
    ret 0;
}
```

The executable path is always at `args[0]`.

## Project structure

After `qz init`, your project looks like:

```
hello/
├── quazi.toml     # package manifest
├── src/
│   └── main.qz   # entry point
└── build/         # output directory (created by qz build)
```

The `quazi.toml` contains package identity and settings:

```toml
[package]
name = "hello"
version = "0.1.0"
std = true
crash_handler = true
mangling = true
```

## Next steps

Now that you have a working program, continue to
[Chapter 2: Variables and types](02-basics.md) to learn about Quazi's type
system and basic operations.
