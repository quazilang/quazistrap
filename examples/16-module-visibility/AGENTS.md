# Example: 16-module-visibility

Demonstrates `pub` visibility enforcement on types in Quazilang.

## Running

```bash
qz build -o pub_types
./pub_types
```

## What is shown

- `pub struct PublicStruct` — can be imported and used across modules
- `struct PrivateStruct` — private; importing it from another module produces a compile error **S04**

## Files

| File | Description |
|------|-------------|
| `src/helper.qz` | Defines a `pub struct PublicStruct` and a private `struct PrivateStruct` |
| `src/main.qz` | Imports and uses `PublicStruct`; shows that importing `PrivateStruct` would fail |

## Uncommenting the private import

In `src/main.qz`, uncomment the line:
```quazi
// import helper.PrivateStruct;
```
and rebuild — the compiler will emit:
```
error[S04]: 'PrivateStruct' is private and cannot be imported
```

This rule applies to `struct`, `enum`, `trait`, and `type` aliases identically to functions.
