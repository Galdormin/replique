//! Parser for Replique

use crate::parser::{ast::NodeDecl, diagnostic::Diagnostics};

pub mod ast;
mod command;
pub mod diagnostic;
mod lines;

pub use ast::parse;

pub const END_NODE_NAME: &str = "END";
pub const RESERVED_NODE_NAMES: &[&str] = &[END_NODE_NAME];

/// Outcome of a parse. There is no `Err`: a broken file still yields the nodes
/// it does contain, alongside the diagnostics explaining what is wrong.
#[derive(Debug)]
pub struct Parsed {
    pub nodes: Vec<NodeDecl>,
    pub diagnostics: Diagnostics,
}

/// Used to index text & tokens in the file (in byte)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    /// Build a span for the start and its length
    pub fn from_length(start: usize, length: usize) -> Self {
        Self {
            start,
            end: start + length,
        }
    }

    /// Return the length in byte of the span
    pub fn length(&self) -> usize {
        self.end - self.start
    }

    /// Smallest span covering both.
    pub fn join(self, other: Span) -> Self {
        Self {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }
}

/// Start offset (in bytes) of every line of a source.
/// Used to turn a [`Span`] into a line and a column.
#[derive(Debug, Clone)]
pub struct LineIndex {
    line_starts: Vec<usize>,
}

impl LineIndex {
    /// Create the line index from the source text.
    pub fn new(src: &str) -> Self {
        let mut line_starts = vec![0];
        line_starts.extend(src.match_indices('\n').map(|(i, _)| i + 1));
        Self { line_starts }
    }

    /// Number of lines of the indexed source. Always at least 1.
    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }

    /// Return the text of a line
    pub fn line_text<'a>(&self, src: &'a str, line: usize) -> &'a str {
        let start = self.line_starts[line];
        if let Some(&end) = self.line_starts.get(line + 1) {
            &src[start..end]
        } else {
            &src[start..]
        }
    }

    /// Line containing `offset`.
    /// An offset past the end of the source belongs to the last line.
    ///
    /// ```rust
    /// use replique::parser::{LineIndex, Span};
    ///
    /// let line_index = LineIndex::new(":= start\nAlice: Hi!\nBob: Hi!\n---");
    /// let alice_span = Span::new(9, 14); // Cover "Alice"
    /// assert_eq!(line_index.line_of(alice_span.start), 1);
    /// ```
    pub fn line_of(&self, offset: usize) -> usize {
        match self.line_starts.binary_search(&offset) {
            Ok(line) => line,
            Err(next) => next - 1,
        }
    }

    /// Byte offset of the start of `line`, clamped to the last line.
    fn line_start(&self, line: usize) -> usize {
        self.line_starts[line.min(self.line_starts.len() - 1)]
    }

    /// Line and column of `offset`
    /// The column is a **byte** offset inside the line.
    ///
    /// ```rust
    /// use replique::parser::{LineIndex, Span};
    ///
    /// let line_index = LineIndex::new(":= start\nAlice: Hi!\nBob: Hi!\n---");
    /// let alice_span = Span::new(9, 14); // Cover "Alice"
    /// assert_eq!(line_index.line_col(alice_span.start), (1, 0));
    /// assert_eq!(line_index.line_col(alice_span.end), (1, 5));
    /// ```
    pub fn line_col(&self, offset: usize) -> (usize, usize) {
        let line = self.line_of(offset);
        (line, offset - self.line_start(line))
    }
}

/// A value paired with its position in the source, in bytes.
#[derive(Debug, PartialEq, Clone, Copy)]
pub struct Spanned<T> {
    /// The value itself.
    pub value: T,
    /// Absolute position of the value in the source, in bytes.
    pub span: Span,
}

impl<'a> Spanned<&'a str> {
    /// Span of `text` once trimmed, where `offset` is the byte position of
    /// `text` (before trimming) in the source.
    fn from_text(text: &'a str, offset: usize) -> Self {
        let rest = text.trim_start();
        let start = offset + text.len() - rest.len();
        let rest = rest.trim();
        Self {
            value: rest,
            span: Span::from_length(start, rest.len()),
        }
    }
}

impl From<Spanned<&str>> for Spanned<String> {
    /// Convert a borrowed slide spanned to a owned string spanned
    fn from(value: Spanned<&str>) -> Self {
        Self {
            value: value.value.into(),
            span: value.span,
        }
    }
}

impl<T> Spanned<T> {
    pub fn into_inner(self) -> T {
        self.value
    }
}
