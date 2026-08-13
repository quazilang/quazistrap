# Example: 17-constant-expressions

Demonstrates compile-time constant folding and cross-basic-block constant propagation in Quazilang.

## Running

```bash
qz build -o constfold
./constfold
```

## Features shown

- Simple constant arithmetic — folding literal math expressions (e.g., 6 * 7) at compile time
- Chained folding — evaluating sequences of dependent constant assignments without runtime overhead
- Dead branch elimination — evaluating constant conditions (0 == 0) at compile time to strip unreachable code
- Cross-block propagation — flowing constant values across basic block boundaries and into loop scopes
- Standard library I/O — converting types and printing output using std.io and std.core
