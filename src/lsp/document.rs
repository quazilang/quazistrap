// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

use tower_lsp::lsp_types::{TextDocumentContentChangeEvent, Url};

use crate::cancel::CancellationToken;
use crate::semantic::SemanticReport;

use super::span::position_to_byte_offset;

pub struct DocumentState {
    pub uri: Url,
    pub source: String,
    pub report: Option<SemanticReport>,
    pub version: i32,
    analysis_cancellation: CancellationToken,
}

impl DocumentState {
    pub fn new(uri: Url, source: String, version: i32) -> Self {
        Self {
            uri,
            source,
            report: None,
            version,
            analysis_cancellation: CancellationToken::new(),
        }
    }

    pub fn update(&mut self, source: String, version: i32) {
        self.analysis_cancellation.cancel();
        self.source = source;
        self.version = version;
        self.report = None;
        self.analysis_cancellation = CancellationToken::new();
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

    pub fn analysis_cancellation(&self) -> CancellationToken {
        self.analysis_cancellation.clone()
    }

    pub fn cancel_analysis(&self) {
        self.analysis_cancellation.cancel();
    }

    /// Apply an LSP incremental-change batch to the currently held document.
    ///
    /// Positions and the optional replacement length use UTF-16 code units,
    /// while Rust strings are UTF-8. Invalid ranges (including a position in
    /// the middle of a surrogate pair) leave the document unchanged so a
    /// malformed client notification cannot corrupt the diagnostic snapshot.
    /// The LSP guarantees that changes in one notification are ordered, so
    /// each later range is resolved against the text produced by its
    /// predecessor.
    pub fn apply_changes_if_newer(
        &mut self,
        changes: &[TextDocumentContentChangeEvent],
        version: i32,
    ) -> Option<String> {
        if version <= self.version {
            return None;
        }

        let mut source = self.source.clone();
        for change in changes {
            source = apply_change(&source, change)?;
        }
        self.update(source.clone(), version);
        Some(source)
    }
}

fn apply_change(source: &str, change: &TextDocumentContentChangeEvent) -> Option<String> {
    let Some(range) = change.range else {
        return Some(change.text.clone());
    };
    let start = position_to_byte_offset(range.start, source)?;
    let end = position_to_byte_offset(range.end, source)?;
    if start > end {
        return None;
    }
    if let Some(expected_length) = change.range_length {
        let actual_length = source[start..end].encode_utf16().count() as u32;
        if actual_length != expected_length {
            return None;
        }
    }

    let mut updated = String::with_capacity(source.len() - (end - start) + change.text.len());
    updated.push_str(&source[..start]);
    updated.push_str(&change.text);
    updated.push_str(&source[end..]);
    Some(updated)
}

#[cfg(test)]
mod tests {
    use super::DocumentState;
    use crate::lexer::Lexer;
    use tower_lsp::lsp_types::{Position, Range, TextDocumentContentChangeEvent, Url};

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

    #[test]
    fn replacing_a_generation_cancels_its_analysis() {
        let uri = Url::parse("file:///workspace/main.qz").expect("test URI");
        let mut document = DocumentState::new(uri, "const value = 1;".to_string(), 1);
        let previous = document.analysis_cancellation();

        document.update("const value = 2;".to_string(), 2);

        assert!(previous.is_cancelled());
        assert!(!document.analysis_cancellation().is_cancelled());
    }

    #[test]
    fn replacing_a_generation_interrupts_its_inflight_lexer() {
        let uri = Url::parse("file:///workspace/main.qz").expect("test URI");
        let mut document = DocumentState::new(uri, "alpha beta gamma".to_string(), 1);
        let cancellation = document.analysis_cancellation();
        let (paused_tx, paused_rx) = std::sync::mpsc::channel();
        let (resume_tx, resume_rx) = std::sync::mpsc::channel();

        let worker = std::thread::spawn(move || {
            let mut lexer = Lexer::new("alpha beta gamma");
            let mut polls = 0;
            lexer.tokenize_with_checkpoint(|| {
                polls += 1;
                if polls == 2 {
                    paused_tx.send(()).expect("worker pauses after one token");
                    resume_rx.recv().expect("worker resumes after replacement");
                }
                cancellation.check()
            })
        });

        paused_rx
            .recv()
            .expect("lexer reaches its cancellation point");
        document.update("const replacement: i32 = 2;".to_string(), 2);
        resume_tx.send(()).expect("resume lexer");

        assert!(matches!(
            worker.join().expect("worker does not panic"),
            Err(_)
        ));
        assert!(!document.is_generation("alpha beta gamma", 1));
    }

    #[test]
    fn applies_ordered_incremental_edits_using_utf16_positions() {
        let uri = Url::parse("file:///workspace/main.qz").expect("test URI");
        let mut document = DocumentState::new(uri, "const icon = \"🚀\";\n".to_string(), 1);
        let changes = vec![
            TextDocumentContentChangeEvent {
                range: Some(Range::new(Position::new(0, 14), Position::new(0, 16))),
                range_length: Some(2),
                text: "🛰️".to_string(),
            },
            TextDocumentContentChangeEvent {
                range: Some(Range::new(Position::new(0, 0), Position::new(0, 0))),
                range_length: Some(0),
                text: "// edited\n".to_string(),
            },
        ];

        let updated = document
            .apply_changes_if_newer(&changes, 2)
            .expect("valid incremental changes");
        assert_eq!(updated, "// edited\nconst icon = \"🛰️\";\n");
        assert!(document.is_generation("// edited\nconst icon = \"🛰️\";\n", 2));
    }

    #[test]
    fn rejects_invalid_incremental_edits_without_advancing_generation() {
        let uri = Url::parse("file:///workspace/main.qz").expect("test URI");
        let mut document = DocumentState::new(uri, "const icon = \"🚀\";".to_string(), 4);
        let invalid = TextDocumentContentChangeEvent {
            range: Some(Range::new(Position::new(0, 14), Position::new(0, 15))),
            range_length: Some(1),
            text: "x".to_string(),
        };

        assert!(document.apply_changes_if_newer(&[invalid], 5).is_none());
        assert!(document.is_generation("const icon = \"🚀\";", 4));
    }

    #[test]
    fn accepts_full_document_replacement_in_incremental_mode() {
        let uri = Url::parse("file:///workspace/main.qz").expect("test URI");
        let mut document = DocumentState::new(uri, "const old = 1;".to_string(), 1);
        let replacement = TextDocumentContentChangeEvent {
            range: None,
            range_length: None,
            text: "const new = 2;".to_string(),
        };

        assert_eq!(
            document.apply_changes_if_newer(&[replacement], 2),
            Some("const new = 2;".to_string())
        );
        assert!(document.is_generation("const new = 2;", 2));
    }

    #[test]
    fn rejects_a_mismatched_utf16_range_length() {
        let uri = Url::parse("file:///workspace/main.qz").expect("test URI");
        let mut document = DocumentState::new(uri, "const value = 1;".to_string(), 1);
        let invalid = TextDocumentContentChangeEvent {
            range: Some(Range::new(Position::new(0, 14), Position::new(0, 15))),
            range_length: Some(2),
            text: "2".to_string(),
        };

        assert!(document.apply_changes_if_newer(&[invalid], 2).is_none());
        assert!(document.is_generation("const value = 1;", 1));
    }
}
