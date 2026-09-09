# Chapter 6: Modules and packages

This chapter covers organizing code across files and using dependencies.

## Modules

Each `.qz` file is a module. Module paths use `.`, never `::`.

### Single item import

```quazi
// tutorial: runnable
import std.io.println;

fn main() i32 {
    println("Hello!");
    ret 0;
}
```

### Module import

```quazi
// tutorial: runnable
import std.io;

fn main() i32 {
    io.println("Hello!");
    ret 0;
}
```

### Multi-item import

```quazi
// tutorial: fragment — requires the surrounding chapter context.
import std.io.{println, eprintln};
```

### Aliased import

```quazi
// tutorial: runnable
import std.io.println as log;

fn main() i32 {
    log("Logged message");
    ret 0;
}
```

## Project layout

A multi-file project uses a `src/` directory:

```
my_project/
├── quazi.toml
├── src/
│   ├── main.qz      # entry point
│   ├── math.qz      # module: math
│   └── utils/
│       ├── mod.qz    # module gateway: utils
│       └── helpers.qz # module: utils.helpers
```

### Writing a module

```quazi
// tutorial: fragment — requires the surrounding chapter context.
// src/math.qz
pub fn square(x: i32) i32 {
    ret x * x;
}

pub fn cube(x: i32) i32 {
    ret x * x * x;
}

fn internal_helper() void {
    // private — not accessible outside this file
}
```

### Using a module

```quazi
// tutorial: fragment — requires the surrounding chapter context.
// src/main.qz
import math.square;
import math.cube;

fn main() i32 {
    io.println("3² = {}", square(3));
    io.println("3³ = {}", cube(3));
    ret 0;
}
```

### Directory modules

A directory becomes a module through `mod.qz`. The gateway controls what is
visible:

```quazi
// tutorial: fragment — requires the surrounding chapter context.
// src/utils/mod.qz
pub import ./helpers.format_name;
```

```quazi
// tutorial: fragment — requires the surrounding chapter context.
// src/utils/helpers.qz
pub fn format_name(first: str, last: str) String {
    ret format("{} {}", first, last);
}
```

```quazi
// tutorial: fragment — requires the surrounding chapter context.
// src/main.qz
import utils.format_name;
```

### Relative imports

`./` forces resolution relative to the current file:

```quazi
// tutorial: fragment — requires the surrounding chapter context.
import ./local_helper.value;
```

## Visibility

- `pub` on a function, type, or constant makes it visible outside its file.
- Without `pub`, declarations are file-private.
- `pub import` both imports and re-exports a name.

## Dependencies

### Local dependencies

Add a local dependency in `quazi.toml`:

```toml
[dependencies]
math_lib = { path = "../math_lib" }
```

Use it:

```quazi
// tutorial: fragment — requires the surrounding chapter context.
import math_lib.factorial;
```

### Git dependencies

```toml
[dependencies]
my_dep = { type = "git", url = "https://example.org/dep.git", version = "v1.0" }
```

### Managing dependencies

```bash
qz add ../local_lib            # add local dependency
qz add https://example.org/lib.git --type git  # add Git dependency
qz remove my_dep               # remove a dependency
qz fetch                       # download and verify
qz deps                        # inspect dependency tree
```

### Lock file

`quazi.lock` records resolved dependency versions and checksums. It should be
committed to version control. The build cache (`build/`) should not.

## Library projects

Create a library with `qz new --lib`:

```bash
qz new --lib my_lib
```

The entry point is `src/lib.qz`. Its `pub` declarations become direct
package exports:

```quazi
// tutorial: fragment — requires the surrounding chapter context.
// src/lib.qz
pub fn factorial(n: i64) i64 {
    if (n <= 1) { ret 1; }
    ret n * factorial(n - 1);
}
```

Consumers import directly:

```quazi
// tutorial: fragment — requires the surrounding chapter context.
import my_lib.factorial;
```

## Next steps

Continue to [Chapter 7: Error handling](07-error-handling.md) to learn about
`Result`, `Option`, and the `?` operator.
