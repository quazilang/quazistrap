# Guide: Projects and build pipeline

Audience: Quazi users.

This guide covers creating projects, configuring builds, managing
dependencies, and understanding the build pipeline.

## Creating a project

### New binary project

```bash
qz new my_app
cd my_app
```

Creates:

```
my_app/
├── .git/
├── .gitignore
├── quazi.toml
└── src/
    └── main.qz
```

### New library project

```bash
qz new --lib my_lib
```

Creates `src/lib.qz` as the entry point instead of `src/main.qz`.

### Initialize in an existing directory

```bash
mkdir existing && cd existing
qz init          # binary project
qz init --lib    # library project
```

## The manifest: `quazi.toml`

```toml
[package]
name = "my_app"
version = "0.1.0"
out_dir = "build"       # output directory (default: build)
std = true              # inject standard library prelude
crash_handler = true    # register crash handler
mangling = true         # module-qualify function names

[lib]
name = "my_lib"         # library identity
path = "src/lib.qz"     # library entry point

[[bin]]
name = "my_app"         # binary name
path = "src/main.qz"    # binary entry point

[dependencies]
math = { path = "../math" }

[cc]
sources = ["native/helper.c"]
include-paths = ["native/include"]
defines = ["FEATURE=1"]
flags = ["-Wall"]

[link]
linker = "builtin"      # builtin, auto, or path
libc = false
objects = ["native/prebuilt.o"]
libraries = ["sqlite3"]
library-paths = ["native/lib"]

[target.x86_64-windows.link]
libraries = ["user32"]
```

### Package settings

| Setting | Default | Effect |
| --- | --- | --- |
| `std` | `true` | Inject prelude and resolve `std` |
| `crash_handler` | `true` | Register crash/signal handler |
| `mangling` | `true` | Module-qualify function names |

Setting `std = false` omits the prelude and all standard library resolution.
This is for bare-metal or minimal programs.

## Build commands

| Command | Effect |
| --- | --- |
| `qz build` | Compile and link |
| `qz build -r` | Build, then run the native artifact |
| `qz build -c` | Compile to object file only |
| `qz build -i` | Compile to QZI bytecode only |
| `qz build -s` | Strip debug symbols from a native artifact |
| `qz run` | Build and execute |
| `qz check` | Parse and analyze without building |
| `qz clean` | Remove the output directory |

There is no release-profile flag yet. In particular, `-r` is the short form
of `--run`, not an optimization setting.

### Target selection

```bash
qz build --target x86_64-linux
qz build --target x86_64-windows
```

### Linker selection

```bash
qz build --linker builtin    # in-process ELF/PE linker
qz build --linker /usr/bin/ld.lld  # external linker
```

The built-in linker handles plain executables. Archives, shared libraries,
and libc require an external linker.

## Dependencies

### Adding dependencies

```bash
qz add ../local_lib                           # local path
qz add https://example.org/lib.git --type git # Git repository
qz add ../lib --alias my_alias               # with alias
```

### Removing dependencies

```bash
qz remove my_dep
```

### Inspecting dependencies

```bash
qz fetch   # download and verify
qz deps    # show dependency tree
```

### Dependency types

| Type | Source |
| --- | --- |
| `path` | Local project, `.qz` file, or `.qzi` library |
| `git` | Git repository (tag, hash, or `latest`) |
| `archive` | Downloaded and extracted archive |
| `source` | Single downloaded `.qz` file |
| `qzi` | Compiled QZI library |

### Lock file

`quazi.lock` records exact dependency resolution. Commit it to version
control. The `build/` directory is the disposable compilation cache.

## Build pipeline

```
source → Loader → Lexer → Parser → Analyzer → Codegen → Backend → .o → Linker → binary
```

1. **Loader** — resolves imports, deduplicates, merges dependency-first.
2. **Lexer** — tokenizes source.
3. **Parser** — builds the AST.
4. **Analyzer** — type checking, semantic validation, layout recording.
5. **Codegen** — emits QZI bytecode.
6. **Backend** — x86-64 native code generation.
7. **Linker** — links objects into an executable.

### Incremental builds

QZC (Quazi Compilation Cache) stores pre-WPO function chunks. On partial
changes, only modified functions are recompiled, then full WPO runs over
everything. Cache hits skip the entire frontend.

```bash
qz build           # cache miss on first build
qz build           # cache hit on unchanged source
qz build --no-incremental  # bypass cache
```

## Multi-artifact projects

A single package can produce a library and multiple binaries:

```toml
[lib]
name = "acme"
path = "src/lib.qz"

[[bin]]
name = "acme_cli"
path = "src/main.qz"

[[bin]]
name = "acme_server"
path = "src/server.qz"
```

Select artifacts:

```bash
qz build --lib
qz build --bin acme_cli
qz build --bin acme_server
```

## Output formats

| Flag | Output |
| --- | --- |
| (default) | Native executable |
| `-i` | QZI bytecode (`.qzi`) |
| `-c` | Native object file (`.o`) |
| `--shared-lib` | Shared library (`.so`/`.dll`) |

## Progress output

Build stages report progress:

```
  ◆ cache lookup    miss
  ◆ frontend        4 files
  ◆ bytecode        12 functions (0 cached)
  ◆ native          x86_64.linux
  ◆ cache write     saved
```

Control output with `--silent`, `--no-progress`, `--no-color`, or
`--no-unicode`.
