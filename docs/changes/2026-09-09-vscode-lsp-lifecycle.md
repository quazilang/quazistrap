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

An isolated VS Code 1.133 extension-development host was also opened on a
`.qz` file with the built `qz` executable on `PATH`. Its extension-host log
recorded activation of `quazilang.quazi` and `quazilang language server
initialized`; shutdown had no disposable-type error. This is an
activation/lifecycle smoke, not a substitute for a real-client feature-request
suite.
