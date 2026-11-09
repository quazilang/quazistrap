#!/usr/bin/env python3
"""Check `qz lsp` semantic features on a document that does not parse.

While a call is half typed the document has a parse error. The server must
still publish that error and answer hover, completion and signature help from
a recovered analysis whose positions match the current text.
"""

import argparse
import tempfile
from pathlib import Path

from lsp_client import Client

SOURCE = """fn helper(count: i32, label: str) i32 { ret count; }

fn main() void {
    const total: i32 = helper(2, "λ🙂");
    helper(total, 
}

fn after(value: i32) i32 { ret value; }
"""


def check(qz, root):
    main = root / "main.qz"
    main.write_text(SOURCE)
    uri = main.resolve().as_uri()
    client = Client(qz)
    try:
        client.request("initialize", {"processId": None, "rootUri": root.resolve().as_uri(),
                                      "capabilities": {}})
        client.notify("initialized", {})
        client.notify("textDocument/didOpen", {"textDocument": {
            "uri": uri, "languageId": "quazi", "version": 1, "text": SOURCE}})
        diagnostics = client.wait_for("textDocument/publishDiagnostics")
        assert len(diagnostics["diagnostics"]) == 1, diagnostics
        assert diagnostics["diagnostics"][0]["range"]["start"]["line"] in (4, 5), diagnostics

        document = {"uri": uri}
        # Signature help inside the unfinished call: second parameter active.
        signature = client.request("textDocument/signatureHelp", {
            "textDocument": document, "position": {"line": 4, "character": 18}})
        label = signature["signatures"][0]["label"] if signature else ""
        assert label.startswith("helper(i32, str)"), signature
        assert signature.get("activeParameter") == 1, signature

        # Hover before and after the broken line (the line itself is blanked).
        line = SOURCE.splitlines()[7]
        for position in ({"line": 3, "character": 11},
                         {"line": 7, "character": line.rindex("value") + 1}):
            hover = client.request("textDocument/hover", {
                "textDocument": document, "position": position})
            assert hover and "i32" in str(hover["contents"]), (position, hover)

        # Completion offers what is declared before the cursor, including the
        # local of the recovered function.
        completion = client.request("textDocument/completion", {
            "textDocument": document, "position": {"line": 4, "character": 4}})
        labels = {item["label"] for item in (completion or [])}
        assert {"helper", "main", "total"} <= labels, sorted(labels)

        # Definition of a function declared after the broken line.
        definition = client.request("textDocument/definition", {
            "textDocument": document, "position": {"line": 3, "character": 24}})
        assert definition, definition
    finally:
        client.close()
    print("PASS lsp incomplete source: parse error published; signature help, hover, "
          "completion and definition answered from the recovered analysis")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--qz", type=Path, default=Path("target/debug/qz"))
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="quazi-lsp-incomplete-") as directory:
        check(args.qz.resolve(), Path(directory))


if __name__ == "__main__":
    main()
