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
use super::{analysis, completion, diagnostics, formatting, goto_def, hover};

pub struct VoidLanguageServer {
    pub client: Client,
    pub documents: Arc<RwLock<HashMap<Url, DocumentState>>>,
}

impl VoidLanguageServer {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            documents: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    async fn reanalyze_and_publish(&self, uri: Url, text: String, version: i32) {
        let result = analysis::analyze_source(&text);
        let mut docs = self.documents.write().await;
        let doc = docs
            .entry(uri.clone())
            .or_insert_with(|| DocumentState::new(uri.clone(), text.clone(), version));
        doc.update(text.clone(), version);

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
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec![".".to_string()]),
                    ..Default::default()
                }),
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

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let pos = params.text_document_position.position;
        let uri = &params.text_document_position.text_document.uri;
        let docs = self.documents.read().await;
        if let Some(doc) = docs.get(uri) {
            return Ok(completion::complete_at(&doc.source, pos));
        }
        Ok(None)
    }

    async fn formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        let docs = self.documents.read().await;
        if let Some(doc) = docs.get(&params.text_document.uri) {
            return Ok(formatting::format_document(&doc.source));
        }
        Ok(None)
    }
}
