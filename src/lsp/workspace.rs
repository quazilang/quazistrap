// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use tower_lsp::lsp_types::{InitializeParams, Url};

use crate::semantic::SemanticReport;

use super::analysis;

/// A parsed on-disk workspace document. Files that cannot be lexed or parsed
/// deliberately have no entry: an old successful snapshot must not advertise
/// declarations that no longer exist on disk.
pub struct IndexedDocument {
    pub source: String,
    pub report: SemanticReport,
}

/// Read-only workspace-symbol index. The LSP keeps unsaved open buffers in a
/// separate map, where they temporarily override entries with the same URI.
#[derive(Default)]
pub struct WorkspaceIndex {
    roots: Vec<PathBuf>,
    documents: HashMap<Url, IndexedDocument>,
}

impl WorkspaceIndex {
    pub fn from_initialize_params(params: &InitializeParams) -> Self {
        let roots = workspace_roots(params);
        let mut index = Self {
            roots,
            documents: HashMap::new(),
        };
        index.rescan();
        index
    }

    pub fn documents(&self) -> &HashMap<Url, IndexedDocument> {
        &self.documents
    }

    pub fn canonical_uri(&self, uri: &Url) -> Option<Url> {
        let path = local_workspace_source(uri, &self.roots)?;
        Url::from_file_path(path).ok()
    }

    /// Resolve the file portion of a `import ./name.symbol` declaration. The
    /// caller still validates the imported symbol and visibility in the target
    /// document; this helper only enforces the workspace path boundary.
    pub fn relative_leaf_target(&self, importer: &Url, leaf: &str) -> Option<Url> {
        let importer = local_workspace_source(importer, &self.roots)?;
        let candidate = importer.parent()?.join(leaf).with_extension("qz");
        self.canonical_uri(&Url::from_file_path(candidate).ok()?)
    }

    /// Rebuild atomically from local disk state. This is intentionally a
    /// caller-controlled operation until watched-file support exists.
    pub fn rescan(&mut self) {
        let mut paths = Vec::new();
        for root in &self.roots {
            collect_sources(root, root, &mut paths);
        }
        paths.sort();
        paths.dedup();

        let mut documents = HashMap::new();
        for path in paths {
            let Some(uri) = Url::from_file_path(&path).ok() else {
                continue;
            };
            let Some(document) = indexed_document_from_path(&path) else {
                continue;
            };
            documents.insert(uri, document);
        }
        self.documents = documents;
    }

    /// Update one indexed file after a successful save. Non-local documents
    /// and files outside the negotiated roots never enter the disk index.
    pub fn update_from_source(&mut self, uri: &Url, source: &str) {
        self.update_from_analysis(uri, source.to_string(), analysis::analyze_source(source));
    }

    /// Install a source snapshot whose compiler result was prepared by the
    /// caller. This lets the LSP perform analysis on its blocking pool without
    /// holding the workspace index lock.
    pub fn update_from_analysis(
        &mut self,
        uri: &Url,
        source: String,
        analysis_result: Result<SemanticReport, String>,
    ) {
        let Some(canonical_uri) = self.canonical_uri(uri) else {
            return;
        };
        match analysis_result {
            Ok(report) => {
                self.documents
                    .insert(canonical_uri, IndexedDocument { source, report });
            }
            Err(_) => {
                self.documents.remove(&canonical_uri);
            }
        }
    }
}

fn workspace_roots(params: &InitializeParams) -> Vec<PathBuf> {
    let uris: Vec<_> = params
        .workspace_folders
        .as_ref()
        .filter(|folders| !folders.is_empty())
        .map(|folders| folders.iter().map(|folder| &folder.uri).collect())
        .unwrap_or_else(|| params.root_uri.iter().collect());

    let mut roots: Vec<_> = uris
        .into_iter()
        .filter_map(|uri| uri.to_file_path().ok())
        .filter_map(|path| fs::canonicalize(path).ok())
        .filter(|path| path.is_dir())
        .collect();
    roots.sort();
    roots.dedup();
    roots
}

fn collect_sources(root: &Path, directory: &Path, paths: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        // Symlinks are skipped rather than canonicalized and followed. This
        // prevents a workspace root from implicitly granting access outside
        // the client-selected directory tree.
        if file_type.is_symlink() {
            continue;
        }
        let path = entry.path();
        if file_type.is_dir() {
            if entry.file_name() != ".git" {
                collect_sources(root, &path, paths);
            }
            continue;
        }
        if !file_type.is_file() || path.extension().and_then(|ext| ext.to_str()) != Some("qz") {
            continue;
        }
        let Ok(path) = fs::canonicalize(path) else {
            continue;
        };
        if path.starts_with(root) {
            paths.push(path);
        }
    }
}

fn local_workspace_source(uri: &Url, roots: &[PathBuf]) -> Option<PathBuf> {
    let path = uri.to_file_path().ok()?;
    if path.extension().and_then(|ext| ext.to_str()) != Some("qz") {
        return None;
    }
    let root = roots.iter().find(|root| path.starts_with(root))?;
    // Do not treat an outside symlink as an alias for a workspace source.
    // Both the URI's lexical path and the resolved target must be beneath a
    // root selected by the client.
    if fs::symlink_metadata(&path).ok()?.file_type().is_symlink() {
        return None;
    }
    let canonical = fs::canonicalize(path).ok()?;
    canonical.starts_with(root).then_some(canonical)
}

fn indexed_document_from_path(path: &Path) -> Option<IndexedDocument> {
    let source = fs::read_to_string(path).ok()?;
    let report = analysis::analyze_source(&source).ok()?;
    Some(IndexedDocument { source, report })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use tower_lsp::lsp_types::{InitializeParams, Url, WorkspaceFolder};

    use crate::lsp::analysis;

    use super::WorkspaceIndex;

    static NEXT_TEMP_DIR: AtomicUsize = AtomicUsize::new(0);

    fn temp_dir(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "quazi_lsp_workspace_{name}_{}_{}",
            std::process::id(),
            NEXT_TEMP_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).expect("create test workspace");
        path
    }

    fn params(root: &PathBuf) -> InitializeParams {
        InitializeParams {
            workspace_folders: Some(vec![WorkspaceFolder {
                uri: Url::from_file_path(root).expect("workspace URI"),
                name: "test".to_string(),
            }]),
            ..Default::default()
        }
    }

    #[test]
    fn indexes_only_parseable_local_quazi_sources() {
        let root = temp_dir("sources");
        fs::write(root.join("first.qz"), "fn alpha() i32 { ret 1; }").expect("first source");
        fs::create_dir(root.join("nested")).expect("nested directory");
        fs::write(root.join("nested/second.qz"), "fn beta() i32 { ret 2; }")
            .expect("second source");
        fs::write(root.join("broken.qz"), "fn broken(").expect("broken source");
        fs::create_dir(root.join(".git")).expect("git directory");
        fs::write(root.join(".git/hidden.qz"), "fn hidden() void { ret; }").expect("hidden source");

        let index = WorkspaceIndex::from_initialize_params(&params(&root));
        let sources: Vec<_> = index
            .documents()
            .keys()
            .filter_map(|uri| uri.to_file_path().ok())
            .collect();
        assert_eq!(sources.len(), 2, "only valid, non-.git sources are indexed");
        assert!(sources.iter().any(|path| path.ends_with("first.qz")));
        assert!(sources.iter().any(|path| path.ends_with("second.qz")));
    }

    #[test]
    fn saved_source_replaces_and_invalid_source_removes_disk_snapshot() {
        let root = temp_dir("save");
        let path = root.join("main.qz");
        fs::write(&path, "fn disk_name() i32 { ret 1; }").expect("source");
        let uri = Url::from_file_path(&path).expect("URI");
        let mut index = WorkspaceIndex::from_initialize_params(&params(&root));
        assert!(index.documents().contains_key(&uri));

        let saved_source = "fn saved_name() i32 { ret 2; }".to_string();
        let saved_analysis = analysis::analyze_source(&saved_source);
        index.update_from_analysis(&uri, saved_source, saved_analysis);
        assert!(
            index
                .documents()
                .get(&uri)
                .expect("saved document")
                .source
                .contains("saved_name")
        );

        index.update_from_source(&uri, "fn broken(");
        assert!(!index.documents().contains_key(&uri));
    }

    #[test]
    fn root_uri_is_used_when_workspace_folders_are_absent() {
        let root = temp_dir("root_uri");
        fs::write(root.join("main.qz"), "fn main() void { ret; }").expect("source");
        let params = InitializeParams {
            root_uri: Some(Url::from_file_path(&root).expect("root URI")),
            ..Default::default()
        };
        assert_eq!(
            WorkspaceIndex::from_initialize_params(&params)
                .documents()
                .len(),
            1
        );
    }

    #[test]
    fn rejects_a_file_uri_outside_the_negotiated_root() {
        let root = temp_dir("boundary_root");
        let outside = temp_dir("boundary_outside");
        let outside_path = outside.join("outside.qz");
        fs::write(&outside_path, "fn outside() void { ret; }").expect("outside source");
        let index = WorkspaceIndex::from_initialize_params(&params(&root));
        let outside_uri = Url::from_file_path(outside_path).expect("outside URI");
        assert!(index.canonical_uri(&outside_uri).is_none());
    }
}
