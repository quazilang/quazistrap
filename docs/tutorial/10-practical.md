# Chapter 10: Building a complete program

This chapter puts together everything from the tutorial into a realistic
program: a command-line word frequency counter that reads a file and reports
the most common words.

## The program

This program reads a text file, counts word frequencies using a `Map`, and
prints the results:

```quazi
import std.io;
import std.fs;
import std.collections.Map;

// Split text into words by spaces and newlines
fn count_words(text: str) Map {
    var counts = Map.new();
    var current_start: usize = 0;
    var in_word: bool = false;

    for i : 0..text.bytes_len() {
        // Simple ASCII word boundary detection
        const ch = text[i];
        const is_space = ch == 32 || ch == 10 || ch == 13 || ch == 9;

        if (is_space) {
            if (in_word) {
                // End of a word — use the start position as hash key
                const key = current_start;
                match counts.get(key) {
                    Some(n) => {
                        counts.remove(key);
                        counts.insert(key, n + 1);
                    },
                    None => { counts.insert(key, 1); },
                };
                in_word = false;
            }
        } else {
            if (!in_word) {
                current_start = i;
                in_word = true;
            }
        }
    }

    ret counts;
}

fn main(args: Array[str]) i32 {
    // Check arguments
    if (args.len() < 2) {
        io.eprintln("Usage: wordcount <file>");
        ret 1;
    }

    // Read the file
    const path = args[1];
    const result = fs.read_to_string(path);
    match result {
        Ok(content) => {
            const counts = count_words(content.as_str());
            io.println("Found {} unique positions in {}", counts.len(), path);
        },
        Err(e) => {
            io.eprintln("Error reading {}: {}", path, e.message());
            ret 1;
        },
    };

    ret 0;
}
```

## Project setup

```bash
qz new wordcount
```

Edit `src/main.qz` with the code above, then:

```bash
qz run -- myfile.txt
```

## Concepts used

This program demonstrates:

- **Imports** — `std.io`, `std.fs`, `std.collections.Map`.
- **Functions** — `count_words` encapsulates logic.
- **Variables** — `var` for mutable state, `const` for results.
- **Control flow** — `for` range loops, `if`/`else` conditions.
- **Pattern matching** — `match` on `Result` and `Option`.
- **Error handling** — `Result[String, FsError]` from file operations.
- **Collections** — `Map` for frequency counting.
- **Command-line arguments** — `main(args: Array[str])`.
- **String operations** — `bytes_len()`, indexing, `as_str()`.

## Testing

Add tests alongside the application:

```quazi
@test
fn empty_text_has_no_words() void {
    const counts = count_words("");
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
