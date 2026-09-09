# Chapter 10: Building a complete program

This chapter puts together everything from the tutorial into a realistic
program: a command-line byte-position indexer that reads a file and reports
how many byte positions it indexed. The current `Map` API supports `usize`
keys and values only, so a word-frequency table requires a future string-keyed
collection API.

## The program

This program reads a text file, records every byte position in a `Map`, and
prints the number of indexed positions:

```quazi
// tutorial: runnable
import std.io;
import std.fs;
import std.collections.Map;

fn index_positions(text: str) Map {
    var counts = Map.new().unwrap();
    for i : 0..text.bytes_len() {
        counts.insert(i, 1).unwrap();
    }

    ret counts;
}

fn main(args: Array[str]) i32 {
    // Check arguments
    if (args.len() < 2) {
        io.errln("Usage: byte-index <file>");
        ret 1;
    }

    const indexed: i32 = match fs.read_to_string(args[1]) {
        Ok(content) => index_positions(content.as_str()).len() as i32,
        Err(_) => -1,
    };
    if (indexed < 0) {
        io.errln("Cannot read input file");
        ret 1;
    }
    io.println("Indexed {} byte positions", indexed);
    ret 0;
}
```

## Project setup

```bash
qz new byte-index
```

Edit `src/main.qz` with the code above, then:

```bash
qz run -- myfile.txt
```

## Concepts used

This program demonstrates:

- **Imports** — `std.io`, `std.fs`, `std.collections.Map`.
- **Functions** — `index_positions` encapsulates logic.
- **Variables** — `var` for mutable state and `const` for results.
- **Control flow** — `for` range loops and `if` conditions.
- **Pattern matching** — `match` on `Result`.
- **Error handling** — `Result[String, FsError]` from file operations.
- **Collections** — `Map` for byte-position indexing.
- **Command-line arguments** — `main(args: Array[str])`.
- **String operations** — `bytes_len()` and `as_str()`.

## Testing

Add tests alongside the application:

```quazi
// tutorial: fragment — requires the surrounding chapter context.
@test
fn empty_text_has_no_positions() void {
    const counts = index_positions("");
    if (counts.len() != 0) {
        panic("expected empty map for empty text");
    }
}
```

Run tests:

```bash
qz test
```

## Building for release

```bash
qz build -r
```

The `-r` flag produces an optimized release build.

## Cross-platform builds

Build for Windows from a Linux host:

```bash
qz build --target x86_64-windows
```

Use `@cfg` for platform-specific code:

```quazi
// tutorial: fragment — requires the surrounding chapter context.
@cfg(target_os="windows")
fn path_separator() str { ret "\\"; }

@cfg(target_os="linux")
fn path_separator() str { ret "/"; }
```

## What to explore next

Now that you have completed the tutorial, explore these resources:

- [Language specification](../language/README.md) — complete language
  reference.
- [Standard-library API](../api/README.md) — every public module documented.
- [Practical guides](../guides/README.md) — projects, testing, FFI, and
  diagnostics.
- [Examples](../../examples/README.md) — 35 runnable example projects.
- [C interoperability](../FFI.md) — calling C code and exporting to C.
- [Tooling](../tooling/README.md) — LSP, Tree-sitter, and editor setup.
