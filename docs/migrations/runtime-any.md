# Migrating away from runtime `any`

Replace each value-bearing `any` according to its intent:

- Static reusable code: introduce a generic parameter such as `fn id[T](x: T) T`.
- Behavior-based polymorphism: accept `dyn Trait` whose methods use concrete
  types. Methods returning `Self` are not callable through a trait object.
- Closures: give the destination or function parameter an exact
  `fn(Parameters) Return` type.
- Foreign callbacks: declare an exact `@repr(C) type Callback = fn(...) ...`
  alias and pass a signature-compatible `@export pub unsafe fn` in unsafe code.
- Dynamic application data: define an explicit enum with one variant per
  supported representation and match it before accessing the payload.

Format APIs may retain this exact form:

```quazi
@format
pub fn print(template: str, ...args: any) void {
    // `args` is intentionally unavailable here; call-site lowering supplies
    // the formatted template through the ordinary parameters.
}
```

For thread callbacks, replace `Thread.spawn(value: any)` usage with a
target-compatible exported callback accepting one `*u8` context argument. The
low-level spawn contract returns handle `0` on failure; do not call `join` with
that value in older compiler/runtime pairs. In this checkpoint, joining zero is
a no-op. The current high-level `Thread.spawn` wrapper is experimental and
does not yet expose a typed creation error.
Distribute a library as source while migrating a public API; QZI v7 rejects a
public runtime `any` signature.
