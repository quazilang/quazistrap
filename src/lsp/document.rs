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
}
