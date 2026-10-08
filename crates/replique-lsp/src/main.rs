use std::sync::atomic::{AtomicBool, Ordering};

use dashmap::DashMap;
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::*;
use tower_lsp_server::{Client, LanguageServer, LspService, Server};

use crate::document::Document;
use crate::schema::{Lookup, Schemas};

mod completion;
mod document;
mod schema;

#[derive(Debug)]
struct RepliqueLanguageServer {
    client: Client,
    documents: DashMap<Uri, Document>,
    /// The `replique.toml` of the projects the documents belong to.
    schemas: Schemas,
    /// Whether the client takes snippets, as announced at `initialize`.
    snippets: AtomicBool,
}

impl RepliqueLanguageServer {
    fn new(client: Client) -> Self {
        Self {
            client,
            documents: DashMap::new(),
            schemas: Schemas::default(),
            snippets: AtomicBool::new(false),
        }
    }

    async fn refresh(&self, uri: Uri, source: String) {
        // A document that is not a file on disk has no project to belong to.
        let lookup = match uri.to_file_path() {
            Some(path) => self.schemas.for_file(&path),
            None => Lookup::Missing,
        };

        let schema = match &lookup {
            Lookup::Found(schema) => Some(schema.as_ref()),
            Lookup::Missing | Lookup::Invalid { .. } => None,
        };
        let doc = Document::new(uri.clone(), source, schema);
        let diags = doc.diagnostics();

        self.documents.insert(uri.clone(), doc);
        self.client.publish_diagnostics(uri, diags, None).await;

        // Said out loud, and once: a schema that does not read checks
        // nothing, which would otherwise look like a project without a fault.
        if let Lookup::Invalid {
            path,
            error,
            fresh: true,
        } = lookup
        {
            self.client
                .show_message(
                    MessageType::WARNING,
                    format!(
                        "{} could not be read, dialogues are not checked against it: {error}",
                        path.display()
                    ),
                )
                .await;
        }
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

        #[allow(deprecated)]
        let roots = match params.workspace_folders {
            Some(folders) if !folders.is_empty() => {
                folders.into_iter().map(|folder| folder.uri).collect()
            }
            _ => params.root_uri.into_iter().collect::<Vec<_>>(),
        };
        self.schemas.set_roots(
            roots
                .iter()
                .filter_map(|uri| uri.to_file_path())
                .map(|path| path.into_owned())
                .collect(),
        );

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
                    // started: `=>`, `>>` and `#` would never offer anything.
                    trigger_characters: Some(vec![">".into(), "$".into(), "#".into()]),
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
