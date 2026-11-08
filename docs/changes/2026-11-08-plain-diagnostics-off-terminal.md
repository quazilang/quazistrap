# Plain diagnostics outside terminals

Audience: language users, tool authors and compiler developers.

`qz` diagnostics used ANSI color escapes whenever `NO_COLOR` and `--no-color`
were absent, including when stderr was a pipe, file or editor task runner.
Tools then saw text such as `\x1b[1;31merror\x1b[0m\x1b[1m[S02]` instead of
`error[S02]`; the `scripts/check_initialization.py` runner failed for that reason.

Diagnostics now keep colors only when stderr is a terminal. `NO_COLOR` and
`--no-color` still disable colors on terminals. Output is otherwise unchanged.

Verification: `python3 scripts/check_initialization.py --qz target/debug/qz`
fails with the previous compiler and passes with this change (57 checks);
`qz check 2>&1 | cat -v` shows no escape sequences.
