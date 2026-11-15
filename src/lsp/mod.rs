// Quazi Programming Language
// Copyright (c) 2026 quazilang
// SPDX-License-Identifier: 0BSD

mod analysis;
mod code_actions;
mod completion;
pub(crate) mod diagnostics;
mod document;
mod formatting;
mod goto_def;
mod hover;
mod inlay_hints;
mod recovery;
mod references;
mod semantic_tokens;
mod server;
mod signature;
mod snapshot_cache;
mod span;
mod symbols;
mod workspace;

use server::VoidLanguageServer;
use tokio::io::{stdin, stdout};
use tower_lsp::{LspService, Server};

pub fn run_lsp_server() {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        // Analysis recurses over the program like `qz check`, which runs on the
        // 8 MiB main stack; tokio's 2 MiB workers overflow on deep std
        // expressions in debug builds.
        .thread_stack_size(16 * 1024 * 1024)
        .build()
        .expect("failed to build tokio runtime")
        .block_on(async {
            let stdin = stdin();
            let stdout = stdout();
            let (service, socket) = LspService::new(VoidLanguageServer::new);
            Server::new(stdin, stdout, socket).serve(service).await;
        });
}
