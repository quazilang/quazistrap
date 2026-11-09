#!/usr/bin/env python3
"""Measure `qz lsp` navigation latency over stdio against documented targets.

A loader-backed request (references, definition, rename) analyzes the whole
import graph once; repeated requests on unchanged inputs must reuse that
snapshot. An unsaved edit of the document must invalidate it.
"""

import argparse
import statistics
import tempfile
import time
from pathlib import Path

from lsp_client import Client

PROGRAM = """import std.io;
import std.math;

fn main() i32 {
    const title: str = "quazi";
    io.println("{}", title);
    io.println("{}", math.hypot(3.0, 4.0));
    ret title.len() as i32;
}
"""

# Targets for a release build of the compiler (docs/tooling/lsp.md).
COLD_LIMIT_MS = 1000.0
WARM_MEDIAN_LIMIT_MS = 20.0


def timed(client, method, params):
    start = time.perf_counter()
    result = client.request(method, params)
    return (time.perf_counter() - start) * 1000.0, result


def check(qz, root):
    (root / "src").mkdir()
    (root / "quazi.toml").write_text('[package]\nname = "lspperf"\n\n[build]\nentry = "src/main.qz"\n')
    main = root / "src/main.qz"
    main.write_text(PROGRAM)
    uri = main.resolve().as_uri()
    client = Client(qz)
    try:
        client.request("initialize", {"processId": None, "rootUri": root.resolve().as_uri(),
                                      "capabilities": {}})
        client.notify("initialized", {})
        client.notify("textDocument/didOpen", {"textDocument": {
            "uri": uri, "languageId": "quazi", "version": 1, "text": PROGRAM}})
        # `title` in `const title`: line 4, character 10.
        params = {"textDocument": {"uri": uri}, "position": {"line": 4, "character": 10},
                  "context": {"includeDeclaration": True}}
        cold, result = timed(client, "textDocument/references", params)
        assert result and len(result) == 3, result
        warm = []
        for _ in range(10):
            elapsed, result = timed(client, "textDocument/references", params)
            assert result and len(result) == 3, result
            warm.append(elapsed)
        definition_ms, definition = timed(client, "textDocument/definition", {
            "textDocument": {"uri": uri}, "position": {"line": 5, "character": 22}})
        assert definition, definition
        warm.append(definition_ms)

        # An unsaved edit adds a use; the cached snapshot must not answer.
        edited = PROGRAM.replace("ret title.len()", "io.println(\"{}\", title);\n    ret title.len()")
        client.notify("textDocument/didChange", {
            "textDocument": {"uri": uri, "version": 2},
            "contentChanges": [{"text": edited}]})
        _, result = timed(client, "textDocument/references", params)
        assert result and len(result) == 4, result
    finally:
        client.close()

    median = statistics.median(warm)
    print(f"lsp references: cold {cold:.1f} ms, warm median {median:.1f} ms "
          f"(max {max(warm):.1f} ms over {len(warm)} requests)")
    return cold, median


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--qz", type=Path, default=Path("target/release/qz"))
    parser.add_argument("--no-targets", action="store_true",
                        help="report latency without enforcing the release-build targets")
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="quazi-lsp-perf-") as directory:
        cold, median = check(args.qz.resolve(), Path(directory))
    if not args.no_targets:
        assert cold <= COLD_LIMIT_MS, f"cold request {cold:.1f} ms > {COLD_LIMIT_MS} ms"
        assert median <= WARM_MEDIAN_LIMIT_MS, \
            f"warm median {median:.1f} ms > {WARM_MEDIAN_LIMIT_MS} ms"
        print(f"PASS lsp performance: cold <= {COLD_LIMIT_MS:.0f} ms, "
              f"warm median <= {WARM_MEDIAN_LIMIT_MS:.0f} ms; unsaved edit invalidates")


if __name__ == "__main__":
    main()
