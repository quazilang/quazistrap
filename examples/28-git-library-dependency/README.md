# Git library dependency

Downloads a Quazi library from Git and calls its recursive `factorial`
function several times.

After `qz-test-lib` is pushed to GitHub:

```sh
qz run
```

Expected factorials include `0! = 1`, `5! = 120`, and `10! = 3628800`.
