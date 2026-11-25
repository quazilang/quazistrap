# Testing

`qz test [filter]` finds `@test` functions in project `src/` and `tests/` files,
compiles their shared program once, then runs every selected test in its own
native process. One panic or crash fails that test without preventing later
tests from running. Generated runners live under `<out_dir>/tests` and are
removed by `qz clean` with the rest of the output directory.

```quazi
@test
fn addition_works() void {
    if (20 + 22 != 42) {
        panic("addition produced the wrong value");
    }
}
```

Tests must use `fn name() void`, take no attribute arguments, have a body, and
cannot be named `main` because that name is reserved for the generated harness.
They remain private unless normal module API needs require `pub`. A test passes
when its process exits successfully; `panic` and nonzero/crash termination fail
it. The optional filter matches the module-qualified test name. `--no-color`
and `--no-unicode` provide plain output.

`--target <target>` builds the runners for another native target (as
`qz build --target`); since they cannot run on the host, it requires
`--no-run`, which builds every selected runner without running it and reports
`built` per test. Runners for a non-host target go to
`<out_dir>/tests/<target>` (for example `build/tests/x86_64-netbsd`), one
executable per test, to be copied to and run on that system; a runner's exit
status is its result. `--no-run` also works for the host target.

Files under `tests/` use module names rooted at `tests`, so
`tests/http/client.qz` contains tests such as `tests.http.client.connects` and
cannot collide with `src/http/client.qz`. Nested `src/` modules keep their path,
so `src/a/math.qz` and `src/b/math.qz` are distinct modules.
