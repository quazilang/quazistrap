# Example: 15-logical

Demonstrates logical and equality operators in Quazilang.

## Running

```bash
qz build -o logical
./logical
```

## Features shown

- `!` (logical NOT) — inverts a boolean value; `!!` double-negation
- `&&` (logical AND) — true only when both operands are true
- `||` (logical OR) — true when at least one operand is true
- `==` (equality) — true when both values are equal
- `!=` (inequality) — true when values are not equal
- Combined expressions mixing `&&`, `||`, `!`, `==`, `!=`
