//! Zed extension declaring the Replique language and starting `replique-lsp`.

use zed_extension_api::{
    self as zed, settings::LspSettings, Command, LanguageServerId, Result, Worktree,
};

const SERVER_BINARY: &str = "replique-lsp";

struct RepliqueExtension;

impl zed::Extension for RepliqueExtension {
    fn new() -> Self {
        Self
    }

    fn language_server_command(
        &mut self,
        language_server_id: &LanguageServerId,
        worktree: &Worktree,
    ) -> Result<Command> {
        // `lsp.replique-lsp.binary` in the Zed settings wins, then the PATH.
        let settings = LspSettings::for_worktree(language_server_id.as_ref(), worktree)
            .ok()
            .and_then(|settings| settings.binary);

        let command = settings
            .as_ref()
            .and_then(|binary| binary.path.clone())
            .or_else(|| worktree.which(SERVER_BINARY))
            .ok_or_else(|| {
                format!(
                    "`{SERVER_BINARY}` not found in PATH. Install it with \
                     `cargo install --path crates/replique-lsp`, or set \
                     `lsp.replique-lsp.binary.path` in your Zed settings."
                )
            })?;

        Ok(Command {
            command,
            args: settings
                .as_ref()
                .and_then(|binary| binary.arguments.clone())
                .unwrap_or_default(),
            env: settings
                .and_then(|binary| binary.env)
                .map(|env| env.into_iter().collect())
                .unwrap_or_default(),
        })
    }
}

zed::register_extension!(RepliqueExtension);
