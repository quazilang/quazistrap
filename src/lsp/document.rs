// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

use tower_lsp::lsp_types::Url;

use crate::semantic::SemanticReport;

pub struct DocumentState {
    pub uri: Url,
    pub source: String,
    pub report: Option<SemanticReport>,
    pub version: i32,
}

impl DocumentState {
    pub fn new(uri: Url, source: String, version: i32) -> Self {
        Self {
            uri,
            source,
            report: None,
            version,
        }
    }

    pub fn update(&mut self, source: String, version: i32) {
        self.source = source;
        self.version = version;
        self.report = None;
    }

    /// Install a full-document notification only when it is newer than
    /// the version currently held for this URI. LSP clients may send requests
    /// concurrently, so accepting an older notification would make every
    /// later analysis and diagnostic stale by construction.
    pub fn update_if_newer(&mut self, source: String, version: i32) -> bool {
        if version <= self.version {
            return false;
        }
        self.update(source, version);
        true
    }

    pub fn is_generation(&self, source: &str, version: i32) -> bool {
        self.version == version && self.source == source
    }
}

#[cfg(test)]
mod tests {
    use super::DocumentState;
    use tower_lsp::lsp_types::Url;

    #[test]
    fn rejects_an_out_of_order_full_document_update() {
        let uri = Url::parse("file:///workspace/main.qz").expect("test URI");
        let mut document = DocumentState::new(uri, "const value = 2;".to_string(), 2);

        assert!(!document.update_if_newer("const value = 1;".to_string(), 1));
        assert!(document.is_generation("const value = 2;", 2));
        assert!(!document.update_if_newer("const other = 2;".to_string(), 2));
        assert!(document.is_generation("const value = 2;", 2));
        assert!(document.update_if_newer("const value = 3;".to_string(), 3));
        assert!(document.is_generation("const value = 3;", 3));
    }
}
