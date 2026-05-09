mod analysis;
mod completion;
mod diagnostics;
mod document;
mod formatting;
mod goto_def;
mod hover;
mod server;
mod span;

use server::VoidLanguageServer;
use tokio::io::{stdin, stdout};
use tower_lsp::{LspService, Server};

pub fn run_lsp_server() {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("failed to build tokio runtime")
        .block_on(async {
            let stdin = stdin();
            let stdout = stdout();
            let (service, socket) = LspService::new(VoidLanguageServer::new);
            Server::new(stdin, stdout, socket).serve(service).await;
        });
}
