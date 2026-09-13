# Tutorial fence verification

Tutorial Quazi examples now begin with a visible `// tutorial:` classification.
`runnable` examples are independently loaded, analyzed, and lowered during the
compiler documentation tests; `fragment` examples identify context supplied by
their chapter; and `invalid` remains available for deliberate diagnostic
examples.

The new gate corrected the Array tutorial to use `Array.new()` and index-based
iteration, which match the current collection API. This documentation-only
change has no language compatibility impact.
