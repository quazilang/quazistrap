// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

//! Reuse of loader-backed snapshots between navigation requests.
//!
//! A snapshot is a function of the text of every file it loaded, of the
//! manifests and lock files its import graph was resolved with, and of which
//! files exist in the directories import resolution looks at (each loaded
//! file's directory up to its project, dependency or std root). An entry is
//! reused only while each of those inputs, read from the current open-document
//! overlays or from disk, is unchanged. Opening a document also clears the
//! cache, because a buffer for a path that does not exist on disk can take
//! part in resolution too.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::analysis::LoadedSnapshot;

/// Snapshots kept at once; each holds the analyzed import graph of one document.
const CAPACITY: usize = 8;

#[derive(Default)]
pub struct SnapshotCache {
    /// Canonical document path -> snapshot, least recently used first.
    entries: Vec<(PathBuf, Arc<LoadedSnapshot>)>,
}

impl SnapshotCache {
    /// The cached snapshot of `path` if none of its inputs changed.
    pub fn get(
        &mut self,
        path: &Path,
        overlays: &HashMap<PathBuf, String>,
    ) -> Option<Arc<LoadedSnapshot>> {
        let index = self.entries.iter().position(|(entry, _)| entry == path)?;
        let (entry_path, snapshot) = self.entries.remove(index);
        if !inputs_unchanged(&snapshot, overlays) {
            return None;
        }
        self.entries.push((entry_path, Arc::clone(&snapshot)));
        Some(snapshot)
    }

    pub fn insert(&mut self, path: PathBuf, snapshot: Arc<LoadedSnapshot>) {
        self.entries.retain(|(entry, _)| *entry != path);
        if self.entries.len() == CAPACITY {
            self.entries.remove(0);
        }
        self.entries.push((path, snapshot));
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

fn inputs_unchanged(snapshot: &LoadedSnapshot, overlays: &HashMap<PathBuf, String>) -> bool {
    let current = |path: &Path| match overlays.get(path) {
        Some(text) => Some(text.clone()),
        None => std::fs::read_to_string(path).ok(),
    };
    snapshot
        .effective_sources
        .iter()
        .chain(snapshot.manifests.iter().map(|(path, text)| (path, text)))
        .all(|(path, text)| current(path).as_deref() == Some(text.as_str()))
        && snapshot
            .directories
            .iter()
            .all(|(directory, listing)| super::analysis::directory_listing(directory) == *listing)
}

#[cfg(test)]
mod tests {
    use super::SnapshotCache;
    use crate::lsp::analysis::analyze_loaded_document;
    use std::collections::HashMap;
    use std::sync::Arc;

    fn project(root: &std::path::Path) -> std::path::PathBuf {
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("quazi.toml"),
            "[package]\nname = \"cached\"\n\n[build]\nentry = \"src/main.qz\"\n",
        )
        .unwrap();
        std::fs::write(root.join("src/util.qz"), "pub fn helper() i32 { ret 1; }\n").unwrap();
        let main = root.join("src/main.qz");
        std::fs::write(
            &main,
            "import util;\n\nfn main() i32 { ret util.helper(); }\n",
        )
        .unwrap();
        main.canonicalize().unwrap()
    }

    fn workspace(name: &str) -> std::path::PathBuf {
        let root =
            std::env::temp_dir().join(format!("quazi_lsp_cache_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        root
    }

    #[test]
    fn reuses_a_snapshot_only_while_every_input_is_unchanged() {
        let directory = workspace("inputs");
        let main = project(&directory);
        let root = directory.canonicalize().unwrap();
        let snapshot = Arc::new(analyze_loaded_document(&main, &HashMap::new()).unwrap());
        assert!(
            snapshot
                .effective_sources
                .contains_key(&root.join("src/util.qz"))
        );
        let mut cache = SnapshotCache::default();
        cache.insert(main.clone(), Arc::clone(&snapshot));

        let hit = cache
            .get(&main, &HashMap::new())
            .expect("unchanged inputs hit");
        assert!(Arc::ptr_eq(&hit, &snapshot));

        // An open buffer with the disk text is still the same input.
        let same = HashMap::from([(main.clone(), std::fs::read_to_string(&main).unwrap())]);
        assert!(cache.get(&main, &same).is_some());

        // An unsaved edit of an imported file invalidates the entry.
        let edited = HashMap::from([(
            root.join("src/util.qz"),
            "pub fn helper() i32 { ret 2; }\n".to_string(),
        )]);
        assert!(cache.get(&main, &edited).is_none());
        cache.insert(main.clone(), Arc::clone(&snapshot));

        // So do disk changes to a loaded file and to the manifest.
        std::fs::write(root.join("src/util.qz"), "pub fn helper() i32 { ret 3; }\n").unwrap();
        assert!(cache.get(&main, &HashMap::new()).is_none());
        let snapshot = Arc::new(analyze_loaded_document(&main, &HashMap::new()).unwrap());
        cache.insert(main.clone(), Arc::clone(&snapshot));
        assert!(cache.get(&main, &HashMap::new()).is_some());
        let manifest = root.join("quazi.toml");
        let text = std::fs::read_to_string(&manifest).unwrap();
        std::fs::write(&manifest, format!("{text}\n")).unwrap();
        assert!(cache.get(&main, &HashMap::new()).is_none());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_created_file_that_shadows_a_resolved_import_invalidates() {
        let directory = workspace("shadow");
        std::fs::create_dir_all(directory.join("src/util")).unwrap();
        std::fs::write(
            directory.join("quazi.toml"),
            "[package]\nname = \"shadow\"\n\n[build]\nentry = \"src/main.qz\"\n",
        )
        .unwrap();
        std::fs::write(directory.join("src/util/mod.qz"), "pub fn helper() i32 { ret 1; }\n")
            .unwrap();
        let main = directory.join("src/main.qz");
        std::fs::write(&main, "import util.{helper};\n\nfn main() i32 { ret helper(); }\n")
            .unwrap();
        let main = main.canonicalize().unwrap();
        let snapshot = Arc::new(analyze_loaded_document(&main, &HashMap::new()).unwrap());
        let mut cache = SnapshotCache::default();
        cache.insert(main.clone(), snapshot);
        assert!(cache.get(&main, &HashMap::new()).is_some());
        // `src/util.qz` takes precedence over `src/util/mod.qz` once it exists.
        std::fs::write(directory.join("src/util.qz"), "pub fn helper() i32 { ret 2; }\n")
            .unwrap();
        assert!(cache.get(&main, &HashMap::new()).is_none());
        // So does a lock file appearing at the project root.
        let snapshot = Arc::new(analyze_loaded_document(&main, &HashMap::new()).unwrap());
        cache.insert(main.clone(), snapshot);
        std::fs::write(directory.join("quazi.lock"), "").unwrap();
        assert!(cache.get(&main, &HashMap::new()).is_none());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn evicts_the_least_recently_used_snapshot() {
        let directory = workspace("eviction");
        let main = project(&directory);
        let snapshot = Arc::new(analyze_loaded_document(&main, &HashMap::new()).unwrap());
        let mut cache = SnapshotCache::default();
        for index in 0..super::CAPACITY {
            cache.insert(
                directory.join(format!("other{index}")),
                Arc::clone(&snapshot),
            );
        }
        cache.insert(main.clone(), Arc::clone(&snapshot));
        assert!(cache.get(&main, &HashMap::new()).is_some());
        assert_eq!(cache.entries.len(), super::CAPACITY);
        assert!(
            !cache
                .entries
                .iter()
                .any(|(path, _)| path.ends_with("other0"))
        );
        std::fs::remove_dir_all(directory).unwrap();
    }
}
