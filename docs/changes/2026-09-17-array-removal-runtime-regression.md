# Generic array removal runtime regression

The compiler test runner now builds and executes a small project that uses the
safe prelude `Array[i32].remove` method. It removes the middle element from a
three-element array and verifies the returned value, new length, and the two
remaining values in their compacted order.

This is intentionally a plain-element runtime regression. The ownership plan
still requires native tests for owned and nested elements, plus an
out-of-bounds execution path, before generic removal is considered fully
proven.
