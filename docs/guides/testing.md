# Guide: Testing

Audience: Quazi users.

This guide covers writing and running tests with `qz test`.

## Writing tests

Mark a function with `@test` to make it a test:

```quazi
@test
fn addition_works() void {
    if (20 + 22 != 42) {
        panic("addition produced the wrong value");
    }
}

@test
fn string_length() void {
    const text: str = "hello";
    if (text.len() != 5) {
        panic("expected length 5");
    }
}
```

### Test requirements

- Must be `fn name() void` — no parameters, no return value.
- Must have a body.
- Cannot be named `main` (reserved for the test harness).
- May be `pub` or private.
- The `@test` attribute takes no arguments.

### Test success and failure

- A test **passes** when its process exits successfully.
- A test **fails** when it panics or terminates abnormally.

Use `panic(message)` to fail a test with a descriptive message:

```quazi
@test
fn division_by_zero_detected() void {
    const result = safe_divide(10, 0);
    if (result.is_some()) {
        panic("expected None for division by zero");
    }
}
```

## Running tests

```bash
qz test
```

This:

1. Discovers `@test` functions in `src/` and `tests/`.
2. Compiles the entire program once.
3. Runs each test in an isolated process.
4. Reports results.

### Filtering tests

```bash
qz test addition       # run tests whose name contains "addition"
qz test math.square     # run tests in a specific module
```

The filter matches the module-qualified test name.

### Output options

```bash
qz test --no-color      # plain output
qz test --no-unicode    # ASCII-only output
```

## Test file organization

### Tests in `src/`

Tests placed alongside source code share the module's scope:

```
src/
├── main.qz
├── math.qz          # contains @test functions
└── utils.qz         # contains @test functions
```

### Tests in `tests/`

A `tests/` directory provides dedicated test files:

```
src/
├── main.qz
├── math.qz
tests/
├── math_test.qz     # module: tests.math_test
└── integration/
    └── full.qz      # module: tests.integration.full
```

Test files under `tests/` get module names rooted at `tests`, so
`tests/http/client.qz` becomes `tests.http.client` and cannot collide
with `src/http/client.qz`.

## Test isolation

Each test runs in its own native process. One test panicking or crashing does
not prevent other tests from running. This provides:

- No shared mutable state between tests.
- Clean resource cleanup per test.
- Crash isolation.

## Patterns and practices

### Testing with Options and Results

```quazi
@test
fn parsing_valid_input() void {
    const result = "42".parse[i32]();
    if (result.is_err()) {
        panic("expected successful parse");
    }
    if (result.unwrap() != 42) {
        panic("expected 42");
    }
}

@test
fn parsing_invalid_input() void {
    const result = "abc".parse[i32]();
    if (result.is_ok()) {
        panic("expected parse failure for 'abc'");
    }
}
```

### Testing struct behavior

```quazi
struct Counter { value: i32, }

impl Counter {
    fn new() Counter { ret Counter { value: 0 }; }
    fn increment(self: Counter) Counter { ret Counter { value: self.value + 1 }; }
}

@test
fn counter_starts_at_zero() void {
    const c = Counter.new();
    if (c.value != 0) {
        panic("expected initial value 0");
    }
}

@test
fn counter_increments() void {
    const c = Counter.new().increment().increment();
    if (c.value != 2) {
        panic("expected value 2 after two increments");
    }
}
```

### Suppressing warnings in tests

Use `@ignore` on test-only helper functions to suppress unused warnings:

```quazi
@ignore
fn test_helper() i32 {
    ret 42;
}
```

## Build artifacts

Test runners are generated under `<out_dir>/tests` and removed by `qz clean`.

## Standard library tests

The standard library uses the same `@test` mechanism. Run standard library
tests for a specific module:

```bash
qz test ini    # run std.ini tests
```
