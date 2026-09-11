use std::sync::atomic::{AtomicBool, Ordering};

use dashmap::DashMap;
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::*;
use tower_lsp_server::{Client, LanguageServer, LspService, Server};

use crate::document::Document;

mod completion;
mod document;

#[derive(Debug)]
struct RepliqueLanguageServer {
    client: Client,
    documents: DashMap<Uri, Document>,
    /// Whether the client takes snippets, as announced at `initialize`.
    snippets: AtomicBool,
}

impl RepliqueLanguageServer {
    fn new(client: Client) -> Self {
        Self {
            client,
            documents: DashMap::new(),
            snippets: AtomicBool::new(false),
        }
    }

    async fn refresh(&self, uri: Uri, source: String) {
        let doc = Document::new(uri.clone(), source);
        let diags = doc.diagnostics();

        self.documents.insert(uri.clone(), doc);
        self.client.publish_diagnostics(uri, diags, None).await;
    }
}

impl LanguageServer for RepliqueLanguageServer {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        let snippets = params
            .capabilities
            .text_document
            .and_then(|caps| caps.completion)
            .and_then(|caps| caps.completion_item)
            .and_then(|item| item.snippet_support)
            .unwrap_or(false);
        self.snippets.store(snippets, Ordering::Relaxed);

        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Options(
                    TextDocumentSyncOptions {
                        open_close: Some(true),
                        change: Some(TextDocumentSyncKind::FULL),
                        ..Default::default()
                    },
                )),
                completion_provider: Some(CompletionOptions {
                    // Without these, the client only asks once a word is
                    // started: `=>` and `>>` would never offer anything.
                    trigger_characters: Some(vec![">".into(), "$".into()]),
                    ..Default::default()
                }),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "Replique server initialized!")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        self.refresh(params.text_document.uri, params.text_document.text)
            .await;
    }

    async fn did_change(&self, mut params: DidChangeTextDocumentParams) {
        if let Some(change) = params.content_changes.pop() {
            self.refresh(params.text_document.uri, change.text).await;
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        self.documents.remove(&params.text_document.uri);
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let position = params.text_document_position;
        let Some(doc) = self.documents.get(&position.text_document.uri) else {
            return Ok(None);
        };

        Ok(Some(CompletionResponse::Array(doc.completions(
            position.position,
            self.snippets.load(Ordering::Relaxed),
        ))))
    }
}

#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(RepliqueLanguageServer::new);
    Server::new(stdin, stdout, socket).serve(service).await;
}
