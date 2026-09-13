# VS Code LSP lifecycle compatibility

Date: 2026-09-09

Audience: VS Code users and tooling maintainers.

The VS Code integration now awaits startup of its `vscode-languageclient` v9
client instead of registering the startup promise as an extension disposable.
The older lifecycle pattern activated the server but caused VS Code to report
`dispose is not a function` when the extension host shut down.

Deactivation now captures the active client, clears the module state, and waits
for the server to stop. Repeated deactivation is therefore harmless and cannot
stop a later client instance through a stale reference.

Compatibility: no Quazi source or LSP protocol behavior changes. VS Code 1.133
and the current `vscode-languageclient` dependency now have a clean activation
and shutdown path.

Verification:

```bash
cd ../vscode-quazi
node tests/extension-lifecycle.test.js
```

The `tests/run-vscode-hover-smoke.sh` command launches an isolated VS Code
1.133 extension-test host, creates a temporary Quazi project directory,
activates the development extension with the selected `qz` executable, and
asks VS Code's hover-provider API for a literal's `i32 = 42` type/value
information and exact range. It passed with the contained server. Zed and
Helix still need their own runtime evidence.
