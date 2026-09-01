// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

use super::document::DocumentState;
use super::{
    analysis, completion, diagnostics, formatting, goto_def, hover, references, semantic_tokens,
    signature, symbols,
};

pub struct VoidLanguageServer {
    pub client: Client,
    pub documents: Arc<RwLock<HashMap<Url, DocumentState>>>,
}

fn workspace_symbols_for_open_documents(
    documents: &HashMap<Url, DocumentState>,
    query: &str,
) -> Vec<SymbolInformation> {
    let mut results = Vec::new();
    for (uri, doc) in documents {
        if let Some(report) = &doc.report {
            results.extend(symbols::workspace_symbols(report, &doc.source, uri, query));
        }
    }
    results.sort_by(|left, right| {
        left.name
            .to_lowercase()
            .cmp(&right.name.to_lowercase())
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.location.uri.as_str().cmp(right.location.uri.as_str()))
    });
    results
}

impl VoidLanguageServer {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            documents: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    async fn reanalyze_and_publish(&self, uri: Url, text: String, version: i32) {
        // Register the full document before analysis. Analysis can be slower than
        // later didChange notifications, so its result is committed only if this
        // exact document generation is still current.
        {
            let mut docs = self.documents.write().await;
            let accepted = match docs.get_mut(&uri) {
                Some(doc) => doc.update_if_newer(text.clone(), version),
                None => {
                    docs.insert(
                        uri.clone(),
                        DocumentState::new(uri.clone(), text.clone(), version),
                    );
                    true
                }
            };
            if !accepted {
                return;
            }
        }

        let result = analysis::analyze_source(&text);
        let mut docs = self.documents.write().await;
        let Some(doc) = docs.get_mut(&uri) else {
            return;
        };
        if !doc.is_generation(&text, version) {
            return;
        }

        match result {
            Ok(report) => {
                let diags = diagnostics::to_lsp_diagnostics(&report, &text);
                doc.report = Some(report);
                drop(docs);
                self.client
                    .publish_diagnostics(uri, diags, Some(version))
                    .await;
            }
            Err(parse_err) => {
                let diag = Diagnostic {
                    range: diagnostics::parse_error_range(&parse_err, &text),
                    severity: Some(DiagnosticSeverity::ERROR),
                    source: Some("quazilang".to_string()),
                    message: diagnostics::strip_ansi(&parse_err),
                    ..Default::default()
                };
                drop(docs);
                self.client
                    .publish_diagnostics(uri, vec![diag], Some(version))
                    .await;
            }
        }
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for VoidLanguageServer {
    async fn initialize(&self, _params: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                definition_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
                rename_provider: Some(OneOf::Left(true)),
                document_symbol_provider: Some(OneOf::Left(true)),
                workspace_symbol_provider: Some(OneOf::Left(true)),
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec![".".to_string()]),
                    ..Default::default()
                }),
                signature_help_provider: Some(SignatureHelpOptions {
                    trigger_characters: Some(vec!["(".to_string()]),
                    retrigger_characters: Some(vec![",".to_string()]),
                    ..Default::default()
                }),
                semantic_tokens_provider: Some(
                    SemanticTokensOptions {
                        legend: semantic_tokens::legend(),
                        range: None,
                        full: Some(SemanticTokensFullOptions::Bool(true)),
                        ..Default::default()
                    }
                    .into(),
                ),
                document_formatting_provider: Some(OneOf::Left(true)),
                ..Default::default()
            },
            server_info: Some(ServerInfo {
                name: "quazi-lsp".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "quazilang language server initialized")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        self.reanalyze_and_publish(
            params.text_document.uri,
            params.text_document.text,
            params.text_document.version,
        )
        .await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        if let Some(change) = params.content_changes.into_iter().last() {
            self.reanalyze_and_publish(
                params.text_document.uri,
                change.text,
                params.text_document.version,
            )
            .await;
        }
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        let docs = self.documents.read().await;
        if let Some(doc) = docs.get(&params.text_document.uri)
            && let Some(report) = &doc.report
        {
            let diags = diagnostics::to_lsp_diagnostics(report, &doc.source);
            self.client
                .publish_diagnostics(params.text_document.uri.clone(), diags, None)
                .await;
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        self.documents
            .write()
            .await
            .remove(&params.text_document.uri);
        self.client
            .publish_diagnostics(params.text_document.uri, Vec::new(), None)
            .await;
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let pos = params.text_document_position_params.position;
        let uri = &params.text_document_position_params.text_document.uri;
        let docs = self.documents.read().await;
        if let Some(doc) = docs.get(uri)
            && let Some(report) = &doc.report
        {
            return Ok(hover::hover_at(report, &doc.source, pos));
        }
        Ok(None)
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let pos = params.text_document_position_params.position;
        let uri = &params.text_document_position_params.text_document.uri;
        let docs = self.documents.read().await;
        if let Some(doc) = docs.get(uri)
            && let Some(report) = &doc.report
        {
            return Ok(goto_def::goto_definition(report, &doc.source, uri, pos));
        }
        Ok(None)
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let uri = &params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let docs = self.documents.read().await;
        let Some(doc) = docs.get(uri) else {
            return Ok(None);
        };
        Ok(doc
            .report
            .as_ref()
            .and_then(|report| references::references_at(report, &doc.source, uri, position)))
    }

    async fn rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
        let uri = &params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let docs = self.documents.read().await;
        let Some(doc) = docs.get(uri) else {
            return Ok(None);
        };
        Ok(doc.report.as_ref().and_then(|report| {
            references::rename_edits(report, &doc.source, uri, position, &params.new_name)
        }))
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let pos = params.text_document_position.position;
        let uri = &params.text_document_position.text_document.uri;
        let docs = self.documents.read().await;
        if let Some(doc) = docs.get(uri) {
            return Ok(completion::complete_with_report(
                &doc.source,
                pos,
                doc.report.as_ref(),
            ));
        }
        Ok(None)
    }

    async fn signature_help(&self, params: SignatureHelpParams) -> Result<Option<SignatureHelp>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;
        let docs = self.documents.read().await;
        let Some(doc) = docs.get(uri) else {
            return Ok(None);
        };
        Ok(doc
            .report
            .as_ref()
            .and_then(|report| signature::signature_help_at(report, &doc.source, position)))
    }

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        let docs = self.documents.read().await;
        let Some(doc) = docs.get(&params.text_document.uri) else {
            return Ok(None);
        };
        Ok(doc.report.as_ref().map(|report| {
            SemanticTokensResult::Tokens(semantic_tokens::tokens_for(report, &doc.source))
        }))
    }

    async fn formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        let docs = self.documents.read().await;
        if let Some(doc) = docs.get(&params.text_document.uri) {
            return Ok(formatting::format_document(&doc.source));
        }
        Ok(None)
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let docs = self.documents.read().await;
        let Some(doc) = docs.get(&params.text_document.uri) else {
            return Ok(None);
        };
        let Some(report) = &doc.report else {
            return Ok(None);
        };
        Ok(Some(DocumentSymbolResponse::Nested(
            symbols::document_symbols(report, &doc.source),
        )))
    }

    async fn symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> Result<Option<Vec<SymbolInformation>>> {
        let docs = self.documents.read().await;
        Ok(Some(workspace_symbols_for_open_documents(
            &docs,
            &params.query,
        )))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::workspace_symbols_for_open_documents;
    use crate::lsp::{analysis::analyze_source, document::DocumentState};
    use tower_lsp::lsp_types::Url;

    #[test]
    fn workspace_symbol_search_aggregates_open_documents_deterministically() {
        let first_uri = Url::parse("file:///workspace/first.qz").expect("first URI");
        let first_source =
            "fn alpha() i32 { ret 1; }\nfn add() i32 { const added = 2; ret added; }";
        let mut first = DocumentState::new(first_uri.clone(), first_source.to_string(), 1);
        first.report = Some(analyze_source(first_source).expect("analyze first source"));

        let second_uri = Url::parse("file:///workspace/second.qz").expect("second URI");
        let second_source = "fn Address() i32 { ret 3; }";
        let mut second = DocumentState::new(second_uri.clone(), second_source.to_string(), 1);
        second.report = Some(analyze_source(second_source).expect("analyze second source"));

        let mut documents = HashMap::new();
        documents.insert(second_uri.clone(), second);
        documents.insert(first_uri.clone(), first);

        let matching = workspace_symbols_for_open_documents(&documents, "AD");
        let names: Vec<_> = matching.iter().map(|symbol| symbol.name.as_str()).collect();
        assert_eq!(names, ["add", "Address"]);
        assert_eq!(matching[0].location.uri, first_uri);
        assert_eq!(matching[1].location.uri, second_uri);

        let all = workspace_symbols_for_open_documents(&documents, "");
        let names: Vec<_> = all.iter().map(|symbol| symbol.name.as_str()).collect();
        assert_eq!(names, ["add", "Address", "alpha"]);
    }
}
