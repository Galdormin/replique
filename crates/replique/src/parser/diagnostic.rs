//! Problems found in a source file, and how to show them to the user.
//!
//! A [`Diagnostic`] pairs a [`DiagnosticKind`] with the [`Span`] it applies to.
//! The parser collects them into [`Diagnostics`], which owns the [`LineIndex`]
//! needed to turn those spans back into line and column numbers, and knows how
//! to render the whole batch.
//!
//! Two renderings are available, both on the individual diagnostic and on the
//! collection: [`Diagnostics::render_short`] emits one `path:line:col` line per
//! diagnostic, meant for editors and terse CLI output, while
//! [`Diagnostics::render`] draws the offending source lines in the `rustc`
//! style, meant for humans reading a terminal:
//!
//! ```text
//! error[duplicate-node-found]: node `start` is declared more than once
//!    --> village.rep:24:1
//!    |
//! 24 | := start
//!    | ^^^^^^^^
//!    |
//!  3 | := start
//!    | -------- first definition here
//! ```
//!
//! The primary [`span`](Diagnostic::span) is underlined with `^`, and every
//! [`Label`] with `-` followed by its message. Columns are counted in
//! characters so the markers stay aligned under accented text, and a span
//! reaching the next lines is drawn on its first line only.
//!
//! ANSI colors are opt-in through [`Color`], so snapshots and log files stay
//! free of escape codes unless they are asked for.

use std::{
    fmt::{self, Write},
    io::IsTerminal,
    path::{Path, PathBuf},
    slice::Iter,
};

use crate::parser::{LineIndex, Span};

/// How bad a diagnostic is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Something suspicious that does not prevent the source from being parsed.
    Warning,
    /// Something that must be fixed before the source can be used.
    Error,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Warning => f.write_str("warning"),
            Self::Error => f.write_str("error"),
        }
    }
}

/// What is wrong with the source.
///
/// Each variant carries its own [`severity`](Self::severity), its stable
/// [`code`](Self::code), and, through [`Display`](fmt::Display), the message
/// shown to the user. The span it applies to lives in the [`Diagnostic`].
#[derive(Debug, Clone, PartialEq)]
pub enum DiagnosticKind {
    /// Indentation of line with \t
    TabIndentation,
    /// Indentation is not complete
    IrregularIndentation,
    /// `---` missing before the next `:=` or before EOF.
    UnclosedNode(String),
    /// `---` while no node is open.
    StrayNodeEnd,
    /// Content outside of any node.
    ContentOutsideNode,
    /// `:=` without a name.
    EmptyNodeName,
    /// Node name that is not `[A-Za-z_][A-Za-z0-9_]*`.
    InvalidNodeName(String),
    /// `=>` without a target.
    EmptyJump,
    /// `->` without a text.
    EmptyChoiceText,
    /// `----`, `==>`, `:==`, ...
    MalformedMarker(String),
    /// A node without any statement.
    EmptyNode,
    /// A choice group with a single option.
    SingleChoice,
    /// A line indented deeper than the block it belongs to.
    UnexpectedIndentation,
    /// `:=` at an indentation level > 0.
    IndentedNodeStart,
    /// `---` at an indentation level > 0.
    IndentedNodeEnd,
    /// Duplicate node found with name
    DuplicateNodeFound(String),
    /// Jump to unknown node
    JumpToUnknownNode(String),
    /// Node name is reserved
    ReservedNodeName(String),
    /// `>>` without a name.
    EmptyCommand,
    /// `>>` followed by something that is not a call: `>> $gold + 1`.
    ExpectedCommand,
    /// Command name that is not `[A-Za-z_][A-Za-z0-9_]*`.
    InvalidCommandName(String),
    /// Command name that the language keeps for itself: `>> await()`.
    ReservedCommandName(String),
    /// `(` never closed.
    UnclosedCall,
    /// An argument that is not a value: `1..2`, `"oups`, ...
    InvalidCommandArgument(String),
    /// Text left after the `)`: `>> command() some text`.
    TrailingAfterCommand,
    /// A `"` opened a string that the line never closes.
    UnterminatedString,
    /// `$` without a name after it.
    EmptyVariableName,
    /// Digits that do not make a number: `1..2`, `1.2.3`, ...
    InvalidNumber(String),
    /// A character that starts no token of an expression.
    UnknownExpressionCharacter(char),
    /// An expression is missing where one is required: `1 +`, `$gold and`, ...
    ExpectedExpression,
    /// `(` never closed inside an expression.
    UnclosedParenthesis,
    /// Two arguments of a function with no `,` between them: `max(1 2)`.
    ExpectedSeparator,
    /// An operator applied to a type it does not take: `not 1`.
    InvalidUnaryOperand {
        op: String,
        expected: String,
        received: String,
    },
    /// An operator applied to two types it does not take: `"a" - 1`.
    InvalidBinaryOperands {
        op: String,
        lhs: String,
        rhs: String,
    },
    /// A bracketed marker never closed: `[let $gold = 1`.
    UnclosedBracket(String),
    /// `[let` followed by something that does not assign: `[let $gold + 1]`.
    ExpectedAssignment,
    /// Text left inside a bracketed marker: `[let $gold = 1 2]`, `[else oups]`.
    UnexpectedTextInBracket,
    /// A condition that is not a `bool`: `[if $gold + 1]`.
    ConditionIsNotABool(String),
    /// `[elif]` or `[else]` while no `[if]` is open.
    StrayBranch(String),
    /// `[elif]` or a second `[else]` after an `[else]`.
    BranchAfterElse(String),
    /// `.attr` on non Var or Func
    AttributeOnNonVariable,
    /// Dict is not closed by `}`
    UnclosedDict,
    /// A dict with colon between key and value `{name "Léon"}`
    ExpectedColon,
    /// A dict with no valid key `{: "Léon"}`
    ExpectedKey,
    /// A dict naming the same key twice `{hp: 1, hp: 2}`
    DuplicateKey(String),
    /// `[break]` or `[continue]` while no `[while]` is open.
    StrayLoopControl(String),
}

impl DiagnosticKind {
    /// Severity attached to this kind. It never depends on the source.
    pub fn severity(&self) -> Severity {
        use DiagnosticKind::*;
        match self {
            TabIndentation
            | IrregularIndentation
            | UnclosedNode(_)
            | StrayNodeEnd
            | ContentOutsideNode
            | EmptyNodeName
            | InvalidNodeName(_)
            | EmptyJump
            | EmptyChoiceText
            | MalformedMarker(_)
            | DuplicateNodeFound(_)
            | JumpToUnknownNode(_)
            | ReservedNodeName(_)
            | EmptyCommand
            | ExpectedCommand
            | InvalidCommandName(_)
            | ReservedCommandName(_)
            | UnclosedCall
            | InvalidCommandArgument(_)
            | UnterminatedString
            | EmptyVariableName
            | InvalidNumber(_)
            | UnknownExpressionCharacter(_)
            | ExpectedExpression
            | UnclosedParenthesis
            | ExpectedSeparator
            | InvalidUnaryOperand { .. }
            | InvalidBinaryOperands { .. }
            | UnclosedBracket(_)
            | ExpectedAssignment
            | UnexpectedTextInBracket
            | ConditionIsNotABool(_)
            | StrayBranch(_)
            | BranchAfterElse(_)
            | AttributeOnNonVariable
            | UnclosedDict
            | ExpectedColon
            | ExpectedKey
            | StrayLoopControl(_) => Severity::Error,

            EmptyNode
            | SingleChoice
            | UnexpectedIndentation
            | IndentedNodeStart
            | IndentedNodeEnd
            | TrailingAfterCommand
            | DuplicateKey(_) => Severity::Warning,
        }
    }

    /// Stable, machine readable code. Used by the tests or LSP.
    pub fn code(&self) -> &'static str {
        use DiagnosticKind::*;
        match self {
            TabIndentation => "tab-indentation",
            IrregularIndentation => "irregular-indentation",
            UnclosedNode { .. } => "unclosed-node",
            StrayNodeEnd => "stray-node-end",
            ContentOutsideNode => "content-outside-node",
            EmptyNodeName => "empty-node-name",
            InvalidNodeName(_) => "invalid-node-name",
            EmptyJump => "empty-jump",
            EmptyChoiceText => "empty-choice-text",
            MalformedMarker(_) => "malformed-marker",
            EmptyNode => "empty-node",
            SingleChoice => "single-choice",
            UnexpectedIndentation => "unexpected-indentation",
            IndentedNodeStart => "indented-node-start",
            IndentedNodeEnd => "indented-node-end",
            DuplicateNodeFound { .. } => "duplicate-node-found",
            JumpToUnknownNode { .. } => "jump-unknown-node",
            ReservedNodeName { .. } => "reserved-node-name",
            EmptyCommand => "empty-command",
            ExpectedCommand => "expected-command",
            InvalidCommandName(_) => "invalid-command-name",
            ReservedCommandName(_) => "reserved-command-name",
            UnclosedCall => "unclosed-call",
            InvalidCommandArgument(_) => "invalid-command-argument",
            TrailingAfterCommand => "trailing-after-command",
            UnterminatedString => "unterminated-string",
            EmptyVariableName => "empty-variable-name",
            InvalidNumber(_) => "invalid-number",
            UnknownExpressionCharacter(_) => "unknown-expression-character",
            ExpectedExpression => "expected-expression",
            UnclosedParenthesis => "unclosed-parenthesis",
            ExpectedSeparator => "expected-arg-separator",
            InvalidUnaryOperand { .. } => "invalid-unary-operand",
            InvalidBinaryOperands { .. } => "invalid-binary-operands",
            UnclosedBracket(_) => "unclosed-bracket",
            ExpectedAssignment => "expected-assignment",
            UnexpectedTextInBracket => "unexpected-text-in-bracket",
            ConditionIsNotABool(_) => "condition-not-a-bool",
            StrayBranch(_) => "stray-branch",
            BranchAfterElse(_) => "branch-after-else",
            AttributeOnNonVariable => "attribute-on-non-variable",
            UnclosedDict => "unclosed-dict",
            ExpectedColon => "expected-colon",
            ExpectedKey => "expected-key",
            DuplicateKey(_) => "duplicate-key",
            StrayLoopControl(_) => "stray-loop-control",
        }
    }
}

impl fmt::Display for DiagnosticKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use DiagnosticKind::*;
        match self {
            TabIndentation => f.write_str("indentation uses a tab, use spaces instead"),
            IrregularIndentation => {
                f.write_str("indentation is not a multiple of the indentation width")
            }
            UnclosedNode(name) => {
                write!(f, "node `{name}` is never closed, expected `---`")
            }
            StrayNodeEnd => f.write_str("`---` found while no node is open"),
            ContentOutsideNode => {
                f.write_str("content found outside of any node, expected `:= <name>` first")
            }
            EmptyNodeName => f.write_str("`:=` is missing a node name"),
            InvalidNodeName(name) => write!(
                f,
                "`{name}` is an invalid node name, expected a letter or `_` followed by letters, digits or `_`",
            ),
            EmptyJump => f.write_str("`=>` is missing a target node"),
            EmptyChoiceText => f.write_str("`->` is missing a choice text"),
            MalformedMarker(marker) => {
                write!(
                    f,
                    "malformed marker `{marker}`, expected `:=`, `---`, `=>`, `->` or `>>`"
                )
            }
            EmptyNode => f.write_str("node has no statement"),
            SingleChoice => f.write_str("choice group has a single option"),
            UnexpectedIndentation => f.write_str("line is indented deeper than its block"),
            IndentedNodeStart => f.write_str("`:=` must not be indented"),
            IndentedNodeEnd => f.write_str("`---` must not be indented"),
            DuplicateNodeFound(name) => write!(f, "node `{name}` is declared more than once"),
            JumpToUnknownNode(name) => write!(f, "jump to unknown node `{name}`"),
            ReservedNodeName(name) => write!(f, "reserved node name `{name}`"),
            EmptyCommand => f.write_str("`>>` is missing a command name"),
            ExpectedCommand => {
                f.write_str("expected a command, like `name()` or `name(1, \"two\")`")
            }
            InvalidCommandName(name) => write!(
                f,
                "`{name}` is an invalid command name, expected a letter or `_` followed by letters, digits or `_`",
            ),
            ReservedCommandName(name) => write!(
                f,
                "`{name}` is a reserved word, expected a command name after it",
            ),
            UnclosedCall => f.write_str("`(` is never closed, expected `)` at the end of the line"),
            InvalidCommandArgument(arg) => write!(
                f,
                "invalid command argument `{arg}`, expected a number, `true`, `false`, a word or a quoted string"
            ),
            TrailingAfterCommand => f.write_str(
                "unexpected text after the end of the command, expected nothing after `)`",
            ),
            UnterminatedString => {
                f.write_str("string is never closed, expected a `\"` before the end of the line")
            }
            EmptyVariableName => f.write_str("`$` is missing a variable name"),
            InvalidNumber(text) => write!(
                f,
                "`{text}` is an invalid number, expected digits with at most one `.`"
            ),
            UnknownExpressionCharacter(c) => {
                write!(f, "unexpected character `{c}` in the expression")
            }
            ExpectedExpression => f.write_str("expected an expression"),
            UnclosedParenthesis => f.write_str("`(` is never closed, expected `)`"),
            ExpectedSeparator => f.write_str("expected `,` or `)` after this function argument"),
            InvalidUnaryOperand {
                op,
                expected,
                received,
            } => write!(
                f,
                "operator `{op}` expects {expected}, received `{received}`"
            ),
            InvalidBinaryOperands { op, lhs, rhs } => write!(
                f,
                "operator `{op}` cannot be applied to `{lhs}` and `{rhs}`"
            ),
            UnclosedBracket(marker) => write!(
                f,
                "`{marker}` is never closed, expected `]` at the end of the line"
            ),
            ExpectedAssignment => f.write_str("expected `$name = <expression>` after `[let`"),
            UnexpectedTextInBracket => f.write_str("unexpected text, expected `]` right after"),
            ConditionIsNotABool(received) => write!(
                f,
                "a condition must be a `bool`, this one is a `{received}`"
            ),
            StrayBranch(marker) => {
                write!(f, "`{marker}` found while no `[if]` is open")
            }
            BranchAfterElse(marker) => write!(f, "`{marker}` found after `[else]`"),
            AttributeOnNonVariable => f.write_str("found `.attr` on non variable or function"),
            UnclosedDict => f.write_str("dict is never closed, expected `} at the end`"),
            ExpectedColon => f.write_str("expected `:` after a key in dict definition"),
            ExpectedKey => f.write_str("expected a str as a key in dict definition"),
            DuplicateKey(key) => write!(
                f,
                "key `{key}` is given more than once, only the last value is kept"
            ),
            StrayLoopControl(marker) => write!(f, "`{marker}` while not `[while]` is open"),
        }
    }
}

/// A secondary span attached to a diagnostic, pointing at the related location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Label {
    /// Where the related location is, as a byte range in the source.
    pub span: Span,
    /// What to say about it, shown right after the `---` marker.
    pub message: String,
}

/// A single problem found in the source.
#[derive(Debug, Clone, PartialEq)]
pub struct Diagnostic {
    /// What is wrong.
    pub kind: DiagnosticKind,
    /// Where it is wrong, as a byte range in the source.
    pub span: Span,
    /// List of secondary informations
    pub labels: Vec<Label>,
}

/// Whether a rendering emits ANSI escape codes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Color {
    /// Plain text: what tests, snapshots and log files want.
    #[default]
    Never,
    /// Always emit escape codes.
    Always,
    /// Emit escape codes when stderr is a terminal, unless `NO_COLOR` is set.
    /// `CLICOLOR_FORCE` overrides both.
    Auto,
}

impl Color {
    /// Resolve [`Auto`](Self::Auto) against the environment.
    fn enabled(self) -> bool {
        match self {
            Self::Never => false,
            Self::Always => true,
            Self::Auto => {
                if std::env::var_os("CLICOLOR_FORCE").is_some_and(|v| !v.is_empty()) {
                    return true;
                }
                std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty())
                    && std::io::stderr().is_terminal()
            }
        }
    }
}

/// Escape codes of one rendered block, every field empty when color is off.
struct Style {
    /// Severity, its code, and the primary carets.
    severity: &'static str,
    /// Message of the diagnostic.
    message: &'static str,
    /// `-->`, `|`, and the line numbers.
    gutter: &'static str,
    /// Markers and messages of the labels.
    label: &'static str,
    reset: &'static str,
}

impl Style {
    fn new(severity: Severity, color: Color) -> Self {
        if !color.enabled() {
            return Self {
                severity: "",
                message: "",
                gutter: "",
                label: "",
                reset: "",
            };
        }

        Self {
            severity: match severity {
                Severity::Error => "\x1b[1;31m",
                Severity::Warning => "\x1b[1;33m",
            },
            message: "\x1b[1m",
            gutter: "\x1b[1;34m",
            label: "\x1b[1;36m",
            reset: "\x1b[0m",
        }
    }
}

/// Number of characters before the byte offset `byte_col` in `text`.
fn char_col(text: &str, byte_col: usize) -> usize {
    text[..byte_col.min(text.len())].chars().count()
}

/// Number of digits of `n`, i.e. the width its decimal form needs.
fn digits(n: usize) -> usize {
    n.to_string().len()
}

impl Diagnostic {
    /// Return the severity of the diagnostic
    pub fn severity(&self) -> Severity {
        self.kind.severity()
    }

    /// Render the diagnostic as a single line, ending with a newline.
    ///
    /// The [`labels`](Self::labels) are dropped; use [`render`](Self::render)
    /// for the full block.
    ///
    /// ```text
    /// file.rep:2:1: error[irregular-indentation]: indentation is not a multiple of the indentation width
    /// ```
    pub fn render_short(
        &self,
        src: &str,
        line_index: &LineIndex,
        path: Option<&Path>,
        color: Color,
    ) -> String {
        let (line, byte_col) = line_index.line_col(self.span.start);
        let col = char_col(line_index.line_text(src, line), byte_col);
        let style = Style::new(self.severity(), color);

        format!(
            "{}{}:{}: {}{}[{}]{}: {}{}{}\n",
            path.map(|p| format!("{}:", p.display()))
                .unwrap_or_default(),
            line + 1,
            col + 1,
            style.severity,
            self.severity(),
            self.kind.code(),
            style.reset,
            style.message,
            self.kind,
            style.reset,
        )
    }

    /// Render the diagnostic over several lines, quoting the offending source
    /// line, underlining [`span`](Self::span) with `^`, then one snippet per
    /// [`Label`] underlined with `-`.
    ///
    /// Every gutter of the block is as wide as its widest line number.
    ///
    /// ```text
    /// error[duplicate-node-found]: node `start` is declared more than once
    ///    --> file.rep:24:1
    ///    |
    /// 24 | := start
    ///    | ^^^^^^^^
    ///    |
    ///  3 | := start
    ///    | -------- first definition here
    /// ```
    pub fn render(
        &self,
        src: &str,
        line_index: &LineIndex,
        path: Option<&Path>,
        color: Color,
    ) -> String {
        debug_assert!(
            self.span.end <= src.len(),
            "diagnostic span is outside the rendered source"
        );

        let (line, byte_col) = line_index.line_col(self.span.start);
        let col = char_col(line_index.line_text(src, line), byte_col);
        let style = Style::new(self.severity(), color);

        // Widest line number of the block, so every gutter lines up.
        let width = self
            .labels
            .iter()
            .map(|label| line_index.line_of(label.span.start))
            .chain([line])
            .map(|line| digits(line + 1))
            .max()
            .unwrap_or(1);
        let gutter = " ".repeat(width);

        let mut out = format!(
            "{}{}[{}]{}: {}{}{}\n{gutter}{} -->{} {}{}:{}\n",
            style.severity,
            self.severity(),
            self.kind.code(),
            style.reset,
            style.message,
            self.kind,
            style.reset,
            style.gutter,
            style.reset,
            path.map(|p| format!("{}:", p.display()))
                .unwrap_or_default(),
            line + 1,
            col + 1,
        );

        snippet(
            &mut out,
            src,
            line_index,
            &style,
            width,
            self.span,
            '^',
            style.severity,
            None,
        );
        for label in &self.labels {
            snippet(
                &mut out,
                src,
                line_index,
                &style,
                width,
                label.span,
                '-',
                style.label,
                Some(&label.message),
            );
        }

        out
    }
}

/// Push the `   |` / `NN | <line>` / `   | ^^^ msg` part of a block.
///
/// A zero-width span still gets one marker, and a span reaching the next lines
/// is cut at the end of its first line.
#[allow(clippy::too_many_arguments)]
fn snippet(
    out: &mut String,
    src: &str,
    line_index: &LineIndex,
    style: &Style,
    width: usize,
    span: Span,
    marker: char,
    marker_style: &str,
    message: Option<&str>,
) {
    let (line, byte_col) = line_index.line_col(span.start);
    let text = line_index.line_text(src, line).trim_end();

    let line_start = span.start - byte_col;
    let end = span.end.clamp(span.start, line_start + text.len());
    let col = char_col(text, byte_col);
    let marker_width = (char_col(text, end - line_start) - col).max(1);

    let gutter = " ".repeat(width);
    let bar = format!("{}|{}", style.gutter, style.reset);

    let _ = writeln!(out, "{gutter} {bar}");
    let _ = writeln!(
        out,
        "{}{:>width$}{} {bar} {text}",
        style.gutter,
        line + 1,
        style.reset,
    );
    let _ = write!(
        out,
        "{gutter} {bar} {}{marker_style}{}",
        " ".repeat(col),
        marker.to_string().repeat(marker_width),
    );
    match message {
        Some(message) => {
            let _ = writeln!(out, " {message}{}", style.reset);
        }
        None => {
            let _ = writeln!(out, "{}", style.reset);
        }
    }
}

/// Every [`Diagnostic`] reported on a single source, in the order they were found.
#[derive(Debug, Clone)]
pub struct Diagnostics {
    diags: Vec<Diagnostic>,
    line_index: LineIndex,
    path: Option<PathBuf>,
}

impl Diagnostics {
    /// Create an empty collection for the source `line_index` was built from.
    pub(super) fn new(line_index: LineIndex) -> Self {
        Self {
            diags: vec![],
            line_index,
            path: None,
        }
    }

    /// Create an empty collection directly from the source. Create LineIndex on its own.
    pub(super) fn from_src(src: &str) -> Self {
        Self::new(LineIndex::new(src))
    }

    /// Attach the path shown by both renderings. Without it, they start directly at the line number.
    #[must_use]
    pub fn with_path(mut self, path: &Path) -> Self {
        self.path = Some(path.into());
        self
    }

    pub fn is_empty(&self) -> bool {
        self.diags.is_empty()
    }

    /// Report a new [`Diagnostic`] at `span`, with no label.
    pub(super) fn push(&mut self, span: Span, kind: DiagnosticKind) {
        self.push_labeled(span, kind, Vec::new());
    }

    /// Report a new [`Diagnostic`] at `span`, pointing at related locations.
    pub(super) fn push_labeled(&mut self, span: Span, kind: DiagnosticKind, labels: Vec<Label>) {
        self.diags.push(Diagnostic { kind, span, labels });
    }

    /// Number of [`Severity::Error`] diagnostics.
    pub fn errors(&self) -> usize {
        self.diags
            .iter()
            .filter(|d| d.severity() == Severity::Error)
            .count()
    }

    /// Number of [`Severity::Warning`] diagnostics.
    pub fn warnings(&self) -> usize {
        self.diags
            .iter()
            .filter(|d| d.severity() == Severity::Warning)
            .count()
    }

    /// Iterate over the diagnostics, in the order they were reported.
    pub fn iter(&self) -> Iter<'_, Diagnostic> {
        self.diags.iter()
    }

    /// Render every diagnostic as a single line, one per line.
    ///
    /// Returns an empty string when there is nothing to report.
    ///
    /// ```text
    /// file.rep:2:1: error[irregular-indentation]: indentation is not a multiple of the indentation width
    /// file.rep:3:5: error[tab-indentation]: indentation uses a tab, use spaces instead
    /// ```
    pub fn render_short(&self, src: &str, color: Color) -> String {
        self.diags
            .iter()
            .map(|d| d.render_short(src, &self.line_index, self.path.as_deref(), color))
            .collect()
    }

    /// Render every diagnostic in full, one block per diagnostic separated by
    /// a blank line.
    ///
    /// ```text
    /// error[irregular-indentation]: indentation is not a multiple of the indentation width
    ///   --> file.rep:2:1
    ///   |
    /// 2 |    Alice: Hi!
    ///   | ^^^
    /// ```
    pub fn render(&self, src: &str, color: Color) -> String {
        self.diags
            .iter()
            .map(|d| d.render(src, &self.line_index, self.path.as_deref(), color))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ":= start\n   Alice: Hi!\n---"
    /// The three spaces indenting `Alice` start at offset 9.
    const SRC: &str = ":= start\n   Alice: Hi!\n---";
    const INDENT: Span = Span { start: 9, end: 12 };

    fn diag(kind: DiagnosticKind, span: Span) -> Diagnostic {
        Diagnostic {
            kind,
            span,
            labels: Vec::new(),
        }
    }

    fn diags(src: &str, kinds: &[(Span, DiagnosticKind)]) -> Diagnostics {
        let mut diags = Diagnostics::new(LineIndex::new(src));
        for (span, kind) in kinds {
            diags.push(*span, kind.clone());
        }
        diags
    }

    /// Drop every `ESC [ ... m` sequence.
    fn strip_ansi(s: &str) -> String {
        let mut out = String::new();
        let mut chars = s.chars();
        while let Some(c) = chars.next() {
            if c == '\u{1b}' {
                for c in chars.by_ref() {
                    if c == 'm' {
                        break;
                    }
                }
            } else {
                out.push(c);
            }
        }
        out
    }

    fn file_path() -> Option<&'static Path> {
        Some(Path::new("file.rep"))
    }

    #[test]
    fn severity_orders_warning_before_error() {
        assert!(Severity::Warning < Severity::Error);
        assert_eq!(Severity::Warning.to_string(), "warning");
        assert_eq!(Severity::Error.to_string(), "error");
    }

    #[test]
    fn diagnostic_severity_delegates_to_kind() {
        let diag = diag(DiagnosticKind::TabIndentation, INDENT);
        assert_eq!(diag.severity(), DiagnosticKind::TabIndentation.severity());
    }

    #[test]
    fn render_short_uses_one_based_line_and_col() {
        let line_index = LineIndex::new(SRC);
        let diag = diag(DiagnosticKind::IrregularIndentation, INDENT);

        assert_eq!(
            diag.render_short(SRC, &line_index, file_path(), Color::Never),
            "file.rep:2:1: error[irregular-indentation]: indentation is not a multiple of the indentation width\n"
        );
    }

    #[test]
    fn render_short_omits_the_path_when_absent() {
        let line_index = LineIndex::new(SRC);
        let diag = diag(DiagnosticKind::IrregularIndentation, INDENT);

        assert_eq!(
            diag.render_short(SRC, &line_index, None, Color::Never),
            "2:1: error[irregular-indentation]: indentation is not a multiple of the indentation width\n"
        );
    }

    #[test]
    fn render_points_at_the_faulty_line() {
        let line_index = LineIndex::new(SRC);
        let diag = diag(DiagnosticKind::IrregularIndentation, INDENT);

        assert_eq!(
            diag.render(SRC, &line_index, file_path(), Color::Never),
            concat!(
                "error[irregular-indentation]: indentation is not a multiple of the indentation width\n",
                "  --> file.rep:2:1\n",
                "  |\n",
                "2 |    Alice: Hi!\n",
                "  | ^^^\n",
            )
        );
    }

    #[test]
    fn render_handles_a_diagnostic_on_the_last_line() {
        let src = ":= start\n---";
        let line_index = LineIndex::new(src);
        let diag = diag(DiagnosticKind::TabIndentation, Span::new(9, 12));

        assert_eq!(
            diag.render(src, &line_index, None, Color::Never),
            concat!(
                "error[tab-indentation]: indentation uses a tab, use spaces instead\n",
                "  --> 2:1\n",
                "  |\n",
                "2 | ---\n",
                "  | ^^^\n",
            )
        );
    }

    /// A zero-width span still gets one marker, otherwise it renders invisibly.
    #[test]
    fn render_handles_an_empty_source() {
        let line_index = LineIndex::new("");
        let diag = diag(DiagnosticKind::TabIndentation, Span::new(0, 0));

        assert_eq!(
            diag.render("", &line_index, None, Color::Never),
            concat!(
                "error[tab-indentation]: indentation uses a tab, use spaces instead\n",
                "  --> 1:1\n",
                "  |\n",
                "1 | \n",
                "  | ^\n",
            )
        );
    }

    #[test]
    fn render_widens_the_gutter_for_large_line_numbers() {
        let src = "\n".repeat(9) + "   Alice: Hi!";
        let line_index = LineIndex::new(&src);
        let diag = diag(DiagnosticKind::IrregularIndentation, Span::new(9, 12));

        let rendered = diag.render(&src, &line_index, None, Color::Never);
        let lines: Vec<_> = rendered.lines().collect();
        assert_eq!(lines[3], "10 |    Alice: Hi!");
        assert_eq!(lines[4], "   | ^^^");
    }

    /// Columns and marker widths are counted in characters, not bytes.
    #[test]
    fn markers_stay_aligned_on_accented_text() {
        let src = "Alice: héhé\n";
        // Span on the second `hé`, which sits after two multi-byte characters.
        let start = src.find("héhé").unwrap() + "hé".len();
        let line_index = LineIndex::new(src);
        let diag = diag(
            DiagnosticKind::EmptyChoiceText,
            Span::from_length(start, "hé".len()),
        );

        let out = diag.render(src, &line_index, file_path(), Color::Never);

        // 9 characters before the span, not the 11 bytes.
        assert!(out.contains("--> file.rep:1:10"), "{out}");
        let caret_line = out.lines().find(|line| line.contains('^')).unwrap();
        assert_eq!(caret_line, format!("  | {}^^", " ".repeat(9)), "{out}");
    }

    /// A span reaching the next lines is drawn on its first line only.
    #[test]
    fn a_span_crossing_lines_is_cut_at_its_first_line() {
        let line_index = LineIndex::new(SRC);
        let diag = diag(DiagnosticKind::IrregularIndentation, Span::new(9, 25));

        let out = diag.render(SRC, &line_index, None, Color::Never);
        let caret_line = out.lines().find(|line| line.contains('^')).unwrap();
        assert_eq!(caret_line, format!("  | {}", "^".repeat(13)), "{out}");
    }

    #[test]
    fn labels_are_rendered_under_their_own_line() {
        let src = ":= start\n---\n:= start\n---\n";
        let line_index = LineIndex::new(src);
        let diag = Diagnostic {
            kind: DiagnosticKind::DuplicateNodeFound("start".into()),
            span: Span::new(13, 21),
            labels: vec![Label {
                span: Span::new(0, 8),
                message: "first definition here".into(),
            }],
        };

        assert_eq!(
            diag.render(src, &line_index, file_path(), Color::Never),
            concat!(
                "error[duplicate-node-found]: node `start` is declared more than once\n",
                "  --> file.rep:3:1\n",
                "  |\n",
                "3 | := start\n",
                "  | ^^^^^^^^\n",
                "  |\n",
                "1 | := start\n",
                "  | -------- first definition here\n",
            )
        );
    }

    /// The gutter is as wide as the widest line number of the whole block,
    /// labels included.
    #[test]
    fn gutter_width_accounts_for_label_lines() {
        let src = "a\nb\nc\nd\ne\nf\ng\nh\ni\nj\nk\n";
        let line_index = LineIndex::new(src);
        let diag = Diagnostic {
            kind: DiagnosticKind::StrayNodeEnd,
            span: Span::new(18, 19),
            labels: vec![Label {
                span: Span::new(0, 1),
                message: "opened here".into(),
            }],
        };

        let out = diag.render(src, &line_index, None, Color::Never);
        let lines: Vec<_> = out.lines().collect();
        assert_eq!(lines[3], "10 | j", "{out}");
        assert_eq!(lines[4], "   | ^", "{out}");
        assert_eq!(lines[6], " 1 | a", "{out}");
        assert_eq!(lines[7], "   | - opened here", "{out}");
    }

    #[test]
    fn color_is_opt_in() {
        let line_index = LineIndex::new(SRC);
        let diag = diag(DiagnosticKind::IrregularIndentation, INDENT);

        let plain = diag.render(SRC, &line_index, file_path(), Color::Never);
        let colored = diag.render(SRC, &line_index, file_path(), Color::Always);

        assert!(!plain.contains('\u{1b}'), "{plain}");
        assert!(
            colored.contains("\u{1b}[1;31merror[irregular-indentation]"),
            "{colored}"
        );
        // Same text once the escape codes are stripped out.
        assert_eq!(strip_ansi(&colored), plain);
    }

    #[test]
    fn counts_split_errors_and_warnings() {
        let diags = diags(
            SRC,
            &[
                (INDENT, DiagnosticKind::IrregularIndentation),
                (INDENT, DiagnosticKind::TabIndentation),
            ],
        );

        assert_eq!(diags.iter().count(), 2);
        assert_eq!(diags.errors() + diags.warnings(), diags.iter().count());
        assert_eq!(diags.errors(), 2);
        assert_eq!(diags.warnings(), 0);
    }

    #[test]
    fn empty_diagnostics_render_to_nothing() {
        let diags = diags(SRC, &[]);

        assert_eq!(diags.errors(), 0);
        assert_eq!(diags.warnings(), 0);
        assert_eq!(diags.render_short(SRC, Color::Never), "");
        assert_eq!(diags.render(SRC, Color::Never), "");
    }

    #[test]
    fn render_short_concatenates_one_line_per_diagnostic() {
        let diags = diags(
            SRC,
            &[
                (INDENT, DiagnosticKind::IrregularIndentation),
                (Span::new(12, 17), DiagnosticKind::TabIndentation),
            ],
        )
        .with_path(file_path().unwrap());

        assert_eq!(
            diags.render_short(SRC, Color::Never),
            concat!(
                "file.rep:2:1: error[irregular-indentation]: indentation is not a multiple of the indentation width\n",
                "file.rep:2:4: error[tab-indentation]: indentation uses a tab, use spaces instead\n",
            )
        );
    }

    #[test]
    fn blocks_are_separated_by_a_blank_line() {
        let diags = diags(
            SRC,
            &[
                (INDENT, DiagnosticKind::IrregularIndentation),
                (Span::new(12, 17), DiagnosticKind::TabIndentation),
            ],
        );

        let out = diags.render(SRC, Color::Never);
        assert_eq!(out.matches("\n\n").count(), 1, "{out}");
        assert!(out.contains("^^^\n\nerror[tab-indentation]"), "{out}");
    }

    #[test]
    fn with_path_is_reflected_in_both_renderings() {
        let diags = diags(SRC, &[(INDENT, DiagnosticKind::IrregularIndentation)]);
        assert!(diags.render_short(SRC, Color::Never).starts_with("2:1:"));
        assert!(diags.render(SRC, Color::Never).contains("  --> 2:1\n"));

        let diags = diags.with_path(file_path().unwrap());
        assert!(
            diags
                .render_short(SRC, Color::Never)
                .starts_with("file.rep:2:1:")
        );
        assert!(
            diags
                .render(SRC, Color::Never)
                .contains("  --> file.rep:2:1\n")
        );
    }

    #[test]
    fn color_applies_to_the_whole_batch() {
        let diags = diags(SRC, &[(INDENT, DiagnosticKind::IrregularIndentation)]);

        assert!(diags.render(SRC, Color::Always).contains('\u{1b}'));
        assert!(diags.render_short(SRC, Color::Always).contains('\u{1b}'));
    }
}
