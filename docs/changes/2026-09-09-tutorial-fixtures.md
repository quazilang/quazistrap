# Compiler-checked tutorial fixtures

Date: 2026-09-09

Audience: tutorial maintainers and contributors.

Each numbered tutorial chapter now has a complete source fixture under
`docs/tutorial/fixtures/`. The compiler test derives the required fixture set
from the chapter filenames, so adding or removing a chapter without its
matching fixture fails the test. It loads every fixture through the normal
import loader, runs semantic analysis, and lowers the program to Quazi
bytecode.

The fixtures are complete programs; explanatory code blocks in tutorial prose
may intentionally be partial. The byte-indexing project in chapter 10 uses the
current `Map` contract (`usize` keys and values), rather than claiming that the
library can already store word strings as keys.

Compatibility: documentation and test coverage only.

Verification:

```bash
cargo test --offline docs::tests::tutorial_fixtures_analyze_and_lower_to_bytecode
```
