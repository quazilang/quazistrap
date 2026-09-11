// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::RwLock;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

use super::document::DocumentState;
use super::workspace::WorkspaceIndex;
use super::{
    analysis, code_actions, completion, diagnostics, formatting, goto_def, hover, inlay_hints,
    references, semantic_tokens, signature, symbols,
};
use crate::cancel::CancellationToken;

pub struct VoidLanguageServer {
    pub client: Client,
    pub documents: Arc<RwLock<HashMap<Url, DocumentState>>>,
    pub workspace: Arc<RwLock<WorkspaceIndex>>,
}

#[derive(Debug)]
enum CancellableTaskError {
    Cancelled,
    Parse(String),
    Failed,
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

fn workspace_symbols(
    workspace: &WorkspaceIndex,
    documents: &HashMap<Url, DocumentState>,
    query: &str,
) -> Vec<SymbolInformation> {
    let mut results = Vec::new();
    let open_workspace_uris: HashSet<_> = documents
        .keys()
        .filter_map(|uri| workspace.canonical_uri(uri))
        .collect();
    for (uri, document) in workspace.documents() {
        if open_workspace_uris.contains(uri) {
            continue;
        }
        results.extend(symbols::workspace_symbols(
            &document.report,
            &document.source,
            uri,
            query,
        ));
    }
    for (uri, document) in documents {
        if workspace.canonical_uri(uri).is_none() {
            continue;
        }
        let Some(report) = &document.report else {
            continue;
        };
        results.extend(symbols::workspace_symbols(
            report,
            &document.source,
            uri,
            query,
        ));
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

fn loader_overlays(documents: &HashMap<Url, DocumentState>) -> HashMap<PathBuf, String> {
    documents
        .iter()
        .filter_map(|(uri, document)| {
            uri.to_file_path()
                .ok()
                .and_then(|path| path.canonicalize().ok())
                .map(|path| (path, document.source.clone()))
        })
        .collect()
}

/// A loader snapshot is usable only while each captured overlay is still open
/// with the same text, and every subsequently open loaded buffer agrees with
/// its effective source. This prevents an asynchronous request from returning
/// locations or edits calculated from superseded overlays.
fn loaded_snapshot_matches_open_documents(
    snapshot: &analysis::LoadedSnapshot,
    overlays: &HashMap<PathBuf, String>,
    documents: &HashMap<Url, DocumentState>,
) -> bool {
    for (path, source) in overlays {
        if !snapshot.effective_sources.contains_key(path) {
            continue;
        }
        let matches_overlay = documents.iter().any(|(uri, document)| {
            uri.to_file_path()
                .ok()
                .and_then(|candidate| candidate.canonicalize().ok())
                .as_ref()
                == Some(path)
                && document.source == *source
        });
        if !matches_overlay {
            return false;
        }
    }
    documents.iter().all(|(uri, document)| {
        let Some(path) = uri
            .to_file_path()
            .ok()
            .and_then(|path| path.canonicalize().ok())
        else {
            return true;
        };
        snapshot
            .effective_sources
            .get(&path)
            .is_none_or(|source| source == &document.source)
    })
}

fn document_matches_source(
    documents: &HashMap<Url, DocumentState>,
    uri: &Url,
    source: &str,
) -> bool {
    documents
        .get(uri)
        .is_some_and(|document| document.source == source)
}

/// Run compiler work away from Tokio's async worker threads. Cancellation of a
/// request can then stop waiting for the snapshot, although the compiler does
/// not yet expose a cooperative cancellation boundary for work already running
/// on the blocking pool.
async fn run_in_blocking_pool<T>(
    work: impl FnOnce() -> T + Send + 'static,
) -> std::result::Result<T, String>
where
    T: Send + 'static,
{
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|error| format!("LSP blocking task failed: {error}"))
}

/// Bridge tower-lsp request cancellation to compiler work on the blocking
/// pool. tower-lsp drops a canceled request future; the guard then signals the
/// token retained by the worker. A worker must check that token and discard
/// any partial result itself.
async fn run_cancellable_in_blocking_pool<T>(
    work: impl FnOnce(CancellationToken) -> std::result::Result<T, CancellableTaskError>
    + Send
    + 'static,
) -> std::result::Result<T, CancellableTaskError>
where
    T: Send + 'static,
{
    let token = CancellationToken::new();
    let guard = token.cancel_on_drop();
    let result = tokio::task::spawn_blocking(move || work(token))
        .await
        .map_err(|_| CancellableTaskError::Failed)?;
    guard.disarm();
    result
}

async fn analyze_source_in_background(
    source: String,
    cancellation: CancellationToken,
) -> std::result::Result<crate::semantic::SemanticReport, CancellableTaskError> {
    run_in_blocking_pool(move || analysis::analyze_source_cancellable(&source, &cancellation))
        .await
        .map_err(|_| CancellableTaskError::Failed)?
        .map_err(|error| match error {
            analysis::CancellableAnalysisError::Cancelled => CancellableTaskError::Cancelled,
            analysis::CancellableAnalysisError::Parse(error) => CancellableTaskError::Parse(error),
            analysis::CancellableAnalysisError::Load(_) => CancellableTaskError::Failed,
        })
}

async fn analyze_loaded_document_cancellable_in_background(
    path: PathBuf,
    overlays: HashMap<PathBuf, String>,
) -> std::result::Result<analysis::LoadedSnapshot, CancellableTaskError> {
    run_cancellable_in_blocking_pool(move |cancellation| {
        cancellation
            .check()
            .map_err(|_| CancellableTaskError::Cancelled)?;
        analysis::analyze_loaded_document_cancellable(&path, &overlays, &cancellation).map_err(
            |error| match error {
                analysis::CancellableAnalysisError::Cancelled => CancellableTaskError::Cancelled,
                analysis::CancellableAnalysisError::Parse(_)
                | analysis::CancellableAnalysisError::Load(_) => CancellableTaskError::Failed,
            },
        )
    })
    .await
}

fn server_capabilities() -> ServerCapabilities {
    ServerCapabilities {
        // The options form is required to negotiate the notifications this
        // server consumes. The legacy integer form can only describe changes.
        text_document_sync: Some(TextDocumentSyncCapability::Options(
            TextDocumentSyncOptions {
                open_close: Some(true),
                change: Some(TextDocumentSyncKind::INCREMENTAL),
                save: Some(TextDocumentSyncSaveOptions::Supported(true)),
                ..Default::default()
            },
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
        inlay_hint_provider: Some(OneOf::Left(true)),
        code_action_provider: Some(
            CodeActionOptions {
                code_action_kinds: Some(vec![CodeActionKind::QUICKFIX]),
                ..Default::default()
            }
            .into(),
        ),
        document_formatting_provider: Some(OneOf::Left(true)),
        ..Default::default()
    }
}

/// Commit a saved source to the workspace index only while its open-document
/// generation is still current. Keep the document read lock while acquiring
/// the workspace write lock: all paths that need both locks use this same
/// document-then-workspace order, avoiding an inversion with workspace-symbol
/// requests while a document update is queued.
async fn update_workspace_from_saved_generation(
    workspace: &RwLock<WorkspaceIndex>,
    documents: &RwLock<HashMap<Url, DocumentState>>,
    uri: &Url,
    source: String,
    version: i32,
    analysis_result: std::result::Result<crate::semantic::SemanticReport, String>,
) -> bool {
    let documents = documents.read().await;
    if !documents
        .get(uri)
        .is_some_and(|document| document.is_generation(&source, version))
    {
        return false;
    }

    let mut workspace = workspace.write().await;
    workspace.update_from_analysis(uri, source, analysis_result);
    true
}

impl VoidLanguageServer {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            documents: Arc::new(RwLock::new(HashMap::new())),
            workspace: Arc::new(RwLock::new(WorkspaceIndex::default())),
        }
    }

    async fn analyze_and_publish(&self, uri: Url, text: String, version: i32, install: bool) {
        // Register a full document before analysis. Incremental edits are
        // installed atomically by `did_change` before reaching here. Analysis
        // can be slower than later notifications, so its result is committed
        // only if this exact document generation is still current.
        if install {
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

        let cancellation = {
            let docs = self.documents.read().await;
            let Some(doc) = docs.get(&uri) else {
                return;
            };
            if !doc.is_generation(&text, version) {
                return;
            }
            doc.analysis_cancellation()
        };
        let result = analyze_source_in_background(text.clone(), cancellation).await;
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
                // Keep the document lock until the notification is queued.
                // Otherwise a close or newer change can queue its clear/new
                // diagnostics first, after which this stale analysis could
                // repopulate the client with an obsolete snapshot.
                self.client
                    .publish_diagnostics(uri, diags, Some(version))
                    .await;
            }
            Err(CancellableTaskError::Parse(parse_err)) => {
                let diag = Diagnostic {
                    range: diagnostics::parse_error_range(&parse_err, &text),
                    severity: Some(DiagnosticSeverity::ERROR),
                    source: Some("quazilang".to_string()),
                    message: diagnostics::strip_ansi(&parse_err),
                    ..Default::default()
                };
                // See the successful-analysis branch: queue this diagnostic
                // before allowing a later generation or close to queue its
                // authoritative replacement.
                self.client
                    .publish_diagnostics(uri, vec![diag], Some(version))
                    .await;
            }
            Err(CancellableTaskError::Cancelled) => {}
            Err(CancellableTaskError::Failed) => {}
        }
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for VoidLanguageServer {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        // Build the snapshot before initialization completes so the first
        // workspace/symbol request sees the client-selected workspace.
        let index = run_in_blocking_pool(move || WorkspaceIndex::from_initialize_params(&params))
            .await
            .map_err(|_| tower_lsp::jsonrpc::Error::internal_error())?;
        *self.workspace.write().await = index;
        Ok(InitializeResult {
            capabilities: server_capabilities(),
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
        self.analyze_and_publish(
            params.text_document.uri,
            params.text_document.text,
            params.text_document.version,
            true,
        )
        .await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        let version = params.text_document.version;
        let text = {
            let mut docs = self.documents.write().await;
            let Some(doc) = docs.get_mut(&uri) else {
                return;
            };
            doc.apply_changes_if_newer(&params.content_changes, version)
        };
        let Some(text) = text else {
            return;
        };

        self.analyze_and_publish(uri, text, version, false).await;
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
        let Some(doc) = docs.get(&params.text_document.uri) else {
            return;
        };
        let source = doc.source.clone();
        let version = doc.version;
        drop(docs);

        let analysis_source = source.clone();
        let Ok(analysis_result) =
            run_in_blocking_pool(move || analysis::analyze_source(&analysis_source)).await
        else {
            self.client
                .log_message(
                    MessageType::ERROR,
                    "LSP workspace index analysis task did not complete; preserving its prior snapshot",
                )
                .await;
            return;
        };
        update_workspace_from_saved_generation(
            &self.workspace,
            &self.documents,
            &params.text_document.uri,
            source,
            version,
            analysis_result,
        )
        .await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        if let Some(document) = self
            .documents
            .write()
            .await
            .remove(&params.text_document.uri)
        {
            document.cancel_analysis();
        }
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
        let (source, report, overlays) = {
            let docs = self.documents.read().await;
            let Some(doc) = docs.get(uri) else {
                return Ok(None);
            };
            let Some(report) = doc.report.clone() else {
                return Ok(None);
            };
            (doc.source.clone(), report, loader_overlays(&docs))
        };
        if let Some(path) = uri.to_file_path().ok()
            && let Ok(snapshot) =
                analyze_loaded_document_cancellable_in_background(path, overlays.clone()).await
        {
            let docs = self.documents.read().await;
            if !document_matches_source(&docs, uri, &source)
                || !loaded_snapshot_matches_open_documents(&snapshot, &overlays, &docs)
            {
                return Ok(None);
            }
            if let Some(definition) = goto_def::loaded_goto_definition(&snapshot, &source, uri, pos)
            {
                return Ok(Some(definition));
            }
        }

        let docs = self.documents.read().await;
        if !document_matches_source(&docs, uri, &source) {
            return Ok(None);
        }
        let workspace = self.workspace.read().await;
        if let Some((target_uri, exported)) =
            goto_def::relative_leaf_import_target(&source, uri, pos, &workspace)
        {
            if let Some((open_uri, open_doc)) = docs.iter().find(|(open_uri, _)| {
                workspace.canonical_uri(open_uri).as_ref() == Some(&target_uri)
            }) {
                return Ok(open_doc.report.as_ref().and_then(|target_report| {
                    goto_def::public_top_level_definition(
                        target_report,
                        &open_doc.source,
                        open_uri,
                        &exported,
                    )
                }));
            }
            if let Some(target) = workspace.documents().get(&target_uri) {
                return Ok(goto_def::public_top_level_definition(
                    &target.report,
                    &target.source,
                    &target_uri,
                    &exported,
                ));
            }
        }
        Ok(goto_def::goto_definition(&report, &source, uri, pos))
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let uri = &params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let (source, overlays) = {
            let docs = self.documents.read().await;
            let Some(doc) = docs.get(uri) else {
                return Ok(None);
            };
            (doc.source.clone(), loader_overlays(&docs))
        };
        let Some(path) = uri.to_file_path().ok() else {
            return Ok(None);
        };
        let Ok(snapshot) =
            analyze_loaded_document_cancellable_in_background(path, overlays.clone()).await
        else {
            return Ok(None);
        };
        let docs = self.documents.read().await;
        if !document_matches_source(&docs, uri, &source)
            || !loaded_snapshot_matches_open_documents(&snapshot, &overlays, &docs)
        {
            return Ok(None);
        }
        Ok(references::loaded_references_at(
            &snapshot,
            &source,
            uri,
            position,
            params.context.include_declaration,
        ))
    }

    async fn rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
        let uri = &params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let (source, overlays) = {
            let docs = self.documents.read().await;
            let Some(doc) = docs.get(uri) else {
                return Ok(None);
            };
            (doc.source.clone(), loader_overlays(&docs))
        };
        let Some(path) = uri.to_file_path().ok() else {
            return Ok(None);
        };
        let Ok(snapshot) =
            analyze_loaded_document_cancellable_in_background(path, overlays.clone()).await
        else {
            return Ok(None);
        };
        {
            let docs = self.documents.read().await;
            if !document_matches_source(&docs, uri, &source)
                || !loaded_snapshot_matches_open_documents(&snapshot, &overlays, &docs)
            {
                return Ok(None);
            }
        }
        let workspace = self.workspace.read().await;
        Ok(references::loaded_rename_edits(
            &snapshot,
            &source,
            uri,
            position,
            &params.new_name,
            |candidate| workspace.canonical_uri(candidate).is_some(),
        ))
    }

    async fn inlay_hint(&self, params: InlayHintParams) -> Result<Option<Vec<InlayHint>>> {
        let docs = self.documents.read().await;
        let Some(doc) = docs.get(&params.text_document.uri) else {
            return Ok(None);
        };
        Ok(doc
            .report
            .as_ref()
            .map(|report| inlay_hints::type_hints(report, &doc.source, params.range)))
    }

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        let docs = self.documents.read().await;
        let Some(doc) = docs.get(&params.text_document.uri) else {
            return Ok(None);
        };
        Ok(doc.report.as_ref().map(|report| {
            code_actions::unused_import_actions(
                report,
                &doc.source,
                &params.text_document.uri,
                params.range,
                &params.context,
            )
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
        let workspace = self.workspace.read().await;
        Ok(Some(workspace_symbols(&workspace, &docs, &params.query)))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::fs;
    use tokio::sync::RwLock;

    use super::{
        loaded_snapshot_matches_open_documents, run_cancellable_in_blocking_pool,
        run_in_blocking_pool, server_capabilities, update_workspace_from_saved_generation,
        workspace_symbols, workspace_symbols_for_open_documents,
    };
    use crate::lsp::workspace::WorkspaceIndex;
    use crate::lsp::{analysis::analyze_source, document::DocumentState};
    use tower_lsp::lsp_types::{
        InitializeParams, TextDocumentSyncCapability, TextDocumentSyncKind,
        TextDocumentSyncSaveOptions, Url, WorkspaceFolder,
    };

    #[test]
    fn advertises_open_close_incremental_and_save_synchronization() {
        let capabilities = server_capabilities();
        let Some(TextDocumentSyncCapability::Options(sync)) = capabilities.text_document_sync
        else {
            panic!("server must advertise explicit text synchronization options");
        };

        assert_eq!(sync.open_close, Some(true));
        assert_eq!(sync.change, Some(TextDocumentSyncKind::INCREMENTAL));
        assert_eq!(
            sync.save,
            Some(TextDocumentSyncSaveOptions::Supported(true))
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn compiler_work_uses_the_blocking_pool() {
        let current_thread = std::thread::current().id();
        let blocking_thread = run_in_blocking_pool(|| std::thread::current().id())
            .await
            .expect("blocking task completes");
        assert_ne!(current_thread, blocking_thread);
    }

    #[tokio::test]
    async fn saved_workspace_update_rejects_a_superseded_document_generation() {
        let root =
            std::env::temp_dir().join(format!("quazi_lsp_save_generation_{}", std::process::id()));
        fs::create_dir_all(&root).expect("create workspace");
        let path = root.join("main.qz");
        let old_source = "fn old_value() i32 { ret 1; }";
        let new_source = "fn new_value() i32 { ret 2; }";
        fs::write(&path, old_source).expect("write workspace source");
        let uri = Url::from_file_path(&path).expect("workspace URI");

        let workspace = RwLock::new(WorkspaceIndex::from_initialize_params(&InitializeParams {
            workspace_folders: Some(vec![WorkspaceFolder {
                uri: Url::from_file_path(&root).expect("workspace root URI"),
                name: "test".to_string(),
            }]),
            ..Default::default()
        }));
        let mut document = DocumentState::new(uri.clone(), old_source.to_string(), 1);
        document.report = Some(analyze_source(old_source).expect("analyze old source"));
        document.update_if_newer(new_source.to_string(), 2);
        let documents = RwLock::new(HashMap::from([(uri.clone(), document)]));

        let committed = update_workspace_from_saved_generation(
            &workspace,
            &documents,
            &uri,
            old_source.to_string(),
            1,
            analyze_source(old_source),
        )
        .await;

        assert!(!committed);
        assert!(
            workspace
                .read()
                .await
                .documents()
                .get(&uri)
                .is_some_and(|document| document.source == old_source)
        );
        fs::remove_dir_all(root).expect("remove workspace");
    }

    #[tokio::test]
    async fn saved_workspace_update_commits_the_current_document_generation() {
        let root = std::env::temp_dir().join(format!(
            "quazi_lsp_current_save_generation_{}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("create workspace");
        let path = root.join("main.qz");
        let source = "fn saved_value() i32 { ret 3; }";
        fs::write(&path, source).expect("write workspace source");
        let uri = Url::from_file_path(&path).expect("workspace URI");

        let workspace = RwLock::new(WorkspaceIndex::from_initialize_params(&InitializeParams {
            workspace_folders: Some(vec![WorkspaceFolder {
                uri: Url::from_file_path(&root).expect("workspace root URI"),
                name: "test".to_string(),
            }]),
            ..Default::default()
        }));
        let mut document = DocumentState::new(uri.clone(), source.to_string(), 4);
        document.report = Some(analyze_source(source).expect("analyze saved source"));
        let documents = RwLock::new(HashMap::from([(uri.clone(), document)]));

        let committed = update_workspace_from_saved_generation(
            &workspace,
            &documents,
            &uri,
            source.to_string(),
            4,
            analyze_source(source),
        )
        .await;

        assert!(committed);
        assert!(
            workspace
                .read()
                .await
                .documents()
                .get(&uri)
                .is_some_and(|document| document.source == source)
        );
        fs::remove_dir_all(root).expect("remove workspace");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dropping_a_cancellable_request_signals_its_blocking_worker() {
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (cancelled_tx, cancelled_rx) = std::sync::mpsc::channel();
        let request = tokio::spawn(run_cancellable_in_blocking_pool(move |token| {
            started_tx.send(()).expect("worker start is observed");
            while !token.is_cancelled() {
                std::thread::yield_now();
            }
            cancelled_tx
                .send(())
                .expect("worker cancellation is observed");
            Ok(())
        }));

        started_rx.recv().expect("worker starts");
        request.abort();
        cancelled_rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .expect("dropped request cancels the worker token");
    }

    #[test]
    fn loaded_snapshot_rejects_a_changed_open_overlay() {
        let root = std::env::temp_dir().join(format!(
            "quazi_lsp_snapshot_generation_{}",
            std::process::id()
        ));
        if root.exists() {
            fs::remove_dir_all(&root).expect("stale test directory is removed");
        }
        fs::create_dir_all(&root).expect("test directory is created");
        let path = root.join("main.qz");
        fs::write(&path, "fn main() void { ret; }\n").expect("test source is written");
        let canonical_path = path.canonicalize().expect("canonical test source");
        let uri = Url::from_file_path(&canonical_path).expect("test source URI");
        let old_source = "fn main() void { ret; }\n".to_string();
        let snapshot = crate::lsp::analysis::LoadedSnapshot {
            report: analyze_source(&old_source).expect("test source analyzes"),
            source_files: Vec::new(),
            effective_sources: HashMap::from([(canonical_path, old_source.clone())]),
        };
        let mut documents = HashMap::from([(
            uri.clone(),
            DocumentState::new(uri, "fn main() void { ret 1; }\n".to_string(), 2),
        )]);
        let overlays = HashMap::from([(
            path.canonicalize().expect("canonical overlay"),
            old_source.clone(),
        )]);
        assert!(!loaded_snapshot_matches_open_documents(
            &snapshot, &overlays, &documents
        ));

        documents.values_mut().next().expect("open document").source = old_source;
        assert!(loaded_snapshot_matches_open_documents(
            &snapshot, &overlays, &documents
        ));
        documents.clear();
        assert!(!loaded_snapshot_matches_open_documents(
            &snapshot, &overlays, &documents
        ));
        fs::remove_dir_all(root).expect("test directory is removed");
    }

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

    #[test]
    fn open_document_overrides_a_matching_workspace_snapshot() {
        let root = std::env::temp_dir().join(format!(
            "quazi_lsp_workspace_overlay_{}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("create workspace");
        let path = root.join("main.qz");
        fs::write(&path, "fn disk_value() i32 { ret 1; }").expect("write disk source");
        let uri = Url::from_file_path(&path).expect("URI");
        let disk_source = "fn disk_value() i32 { ret 1; }";
        let open_source = "fn buffer_value() i32 { ret 2; }";
        let mut open = DocumentState::new(uri.clone(), open_source.to_string(), 1);
        open.report = Some(analyze_source(open_source).expect("analyze open source"));
        let mut documents = HashMap::new();
        documents.insert(uri.clone(), open);

        let workspace = WorkspaceIndex::from_initialize_params(&InitializeParams {
            workspace_folders: Some(vec![WorkspaceFolder {
                uri: Url::from_file_path(&root).expect("workspace URI"),
                name: "test".to_string(),
            }]),
            ..Default::default()
        });
        assert!(
            workspace
                .documents()
                .get(&uri)
                .expect("disk snapshot")
                .source
                .contains(disk_source)
        );
        let results = workspace_symbols(&workspace, &documents, "value");
        let names: Vec<_> = results.iter().map(|symbol| symbol.name.as_str()).collect();
        assert_eq!(names, ["buffer_value"]);
        fs::remove_dir_all(root).expect("remove test workspace");
    }

    #[test]
    fn workspace_symbol_search_excludes_open_documents_outside_the_root() {
        let root = std::env::temp_dir().join(format!(
            "quazi_lsp_workspace_boundary_root_{}",
            std::process::id()
        ));
        let outside = std::env::temp_dir().join(format!(
            "quazi_lsp_workspace_boundary_outside_{}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("create root");
        fs::create_dir_all(&outside).expect("create outside directory");
        fs::write(root.join("main.qz"), "fn inside_value() i32 { ret 1; }")
            .expect("write root source");
        let outside_path = outside.join("outside.qz");
        let outside_source = "fn outside_value() i32 { ret 2; }";
        fs::write(&outside_path, outside_source).expect("write outside source");
        let outside_uri = Url::from_file_path(outside_path).expect("outside URI");
        let mut outside_document =
            DocumentState::new(outside_uri.clone(), outside_source.into(), 1);
        outside_document.report =
            Some(analyze_source(outside_source).expect("analyze outside source"));
        let mut documents = HashMap::new();
        documents.insert(outside_uri, outside_document);
        let workspace = WorkspaceIndex::from_initialize_params(&InitializeParams {
            workspace_folders: Some(vec![WorkspaceFolder {
                uri: Url::from_file_path(&root).expect("root URI"),
                name: "test".to_string(),
            }]),
            ..Default::default()
        });

        let results = workspace_symbols(&workspace, &documents, "value");
        let names: Vec<_> = results.iter().map(|symbol| symbol.name.as_str()).collect();
        assert_eq!(names, ["inside_value"]);
        fs::remove_dir_all(root).expect("remove root");
        fs::remove_dir_all(outside).expect("remove outside directory");
    }
}
