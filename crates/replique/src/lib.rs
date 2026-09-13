//! Core crate for replique

use std::path::{Path, PathBuf};

use crate::{
    dialogue::{Dialogue, compiler::compile},
    parser::{
        diagnostic::{Color, Diagnostics},
        parse,
    },
};

pub mod dialogue;
pub mod host;
pub mod parser;
pub mod vm;

#[derive(Debug, Clone)]
pub struct RepliqueFile {
    pub path: Option<PathBuf>,
    pub source: String,
    pub dialogue: Option<Dialogue>,
    pub diagnostics: Diagnostics,
}

impl RepliqueFile {
    pub fn from_source(source: impl Into<String>) -> Self {
        let source = source.into();
        let compiled = compile(parse(&source));
        Self {
            path: None,
            source,
            dialogue: compiled.dialogue,
            diagnostics: compiled.diagnostics,
        }
    }

    pub fn with_path(mut self, path: &Path) -> Self {
        self.path = Some(path.into());
        self.diagnostics = self.diagnostics.with_path(path);
        self
    }

    /// Diagnostics rendered for a terminal
    pub fn render_diagnostics(&self, color: Color) -> String {
        self.diagnostics.render(&self.source, color)
    }

    /// Diagnostics rendered as one line each, for an editor or a CI log.
    pub fn render_diagnostics_short(&self, color: Color) -> String {
        self.diagnostics.render_short(&self.source, color)
    }

    pub fn has_errors(&self) -> bool {
        self.diagnostics.errors() > 0
    }
}
