# Loader-backed LSP definitions

Go-to-definition now follows semantic bindings through the compiler's
configured local, package, and standard-library import graph. The LSP supplies
canonical open-document buffers as loader overlays, so navigation uses unsaved
importer and target text rather than stale disk contents.

The loader now retains the effective source text for each canonical loaded
file. The LSP uses that source map to translate merged compiler spans back into
per-file UTF-16 locations. This avoids assuming disk text matches the loader's
compatibility-filtered source and supports definitions after Unicode text.

The feature does not change the language or package semantics. It reuses the
same resolver, public-export checks, target conditions, and package settings as
normal compilation. Cross-file references and rename remain unsupported.

Regression coverage includes local modules, a local package dependency,
standard-library imports, UTF-16 position rebasing, and an unsaved imported
document overlay.
