# Tutorial fixtures

These complete programs are the compiler-checked companions for the numbered
tutorial chapters. They deliberately contain only behavior taught by their
chapter or earlier chapters; explanatory snippets in the chapter prose may be
partial and are not independently wrapped or inferred by the verifier.

The compiler test suite discovers every `.qz` entry fixture in this directory,
loads its imports, performs semantic analysis, and lowers it to Quazi bytecode.
Each entry fixture is named after its chapter; multi-file fixtures use
`main.qz` as their entry point.
