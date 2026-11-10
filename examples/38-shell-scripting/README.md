# Shell-style scripting

Lists the subdirectories of a directory with `fs.read_dir` and runs
`git status --short` in each through `process.Command` (PATH lookup, working
directory, environment), printing whether each one is a repository.

```bash
qz run -- /path/to/workspace
```
