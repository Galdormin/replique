use replique::parser::{
    LineIndex, Span,
    ast::NodeDecl,
    diagnostic::{Diagnostic, Diagnostics, Severity},
    parse,
};
use tower_lsp_server::ls_types;

#[derive(Debug)]
pub struct Document {
    pub uri: ls_types::Uri,
    pub source: String,
    pub line_index: LineIndex,
    #[allow(dead_code)]
    pub nodes: Vec<NodeDecl>,
    pub diagnostics: Diagnostics,
}

impl Document {
    pub fn new(uri: ls_types::Uri, source: String) -> Self {
        let line_index = LineIndex::new(&source);
        let parsed = parse(&source);

        Self {
            uri,
            source,
            line_index,
            nodes: parsed.nodes,
            diagnostics: parsed.diagnostics,
        }
    }

    pub fn diagnostics(&self) -> Vec<ls_types::Diagnostic> {
        self.diagnostics
            .iter()
            .map(|d| self.diag_to_ls_diag(d))
            .collect()
    }

    /// Convert a [`Diagnostic`] to a [`ls_types::Diagnostic`]
    pub fn diag_to_ls_diag(&self, diag: &Diagnostic) -> ls_types::Diagnostic {
        let related = diag
            .labels
            .iter()
            .map(|l| ls_types::DiagnosticRelatedInformation {
                location: ls_types::Location::new(self.uri.clone(), self.span_to_range(l.span)),
                message: l.message.clone(),
            })
            .collect::<Vec<_>>();

        ls_types::Diagnostic {
            range: self.span_to_range(diag.span),
            severity: Some(match diag.kind.severity() {
                Severity::Warning => ls_types::DiagnosticSeverity::WARNING,
                Severity::Error => ls_types::DiagnosticSeverity::ERROR,
            }),
            code: Some(diag.kind.code().into()),
            source: Some("replique-lsp".into()),
            message: diag.kind.to_string(),
            related_information: Some(related),
            ..Default::default()
        }
    }

    /// Convert an offset on byte source to a [`ls_types::Position`] in UTF-16
    pub fn offset_to_position(&self, byte_offset: usize) -> ls_types::Position {
        let (line, byte_col) = self.line_index.line_col(byte_offset);
        let text = self.line_index.line_text(&self.source, line);

        let byte_col = byte_col.min(text.len());
        let utf16_col = text[..byte_col].encode_utf16().count();

        ls_types::Position {
            line: line as u32,
            character: utf16_col as u32,
        }
    }

    /// Convert a [`Span`] to [`ls_types::Range`] in UTF-16
    pub fn span_to_range(&self, span: Span) -> ls_types::Range {
        ls_types::Range::new(
            self.offset_to_position(span.start),
            self.offset_to_position(span.end),
        )
    }
}
