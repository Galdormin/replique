//! First pass of the parser: split the source into raw lines and identify the
//! kind of each one.
//!
//! Every line of the source is trimmed, stripped of its comment, and classified
//! into a [`LineKind`] based on its leading marker (`:=`, `---`, `->`, `=>`,
//! `>>`, or a `speaker:` prefix). The payload that follows the marker is kept as
//! a raw [`Spanned`] slice: this pass only decides *what* a line is, it does not
//! parse its content. That work is left to the lexer, which runs on the
//! [`RawLine`] produced here.

use crate::parser::{
    Span, Spanned,
    diagnostic::{DiagnosticKind, Diagnostics},
};

/// Kind of a line, along with the part of the line that follows its marker.
///
/// The payloads are raw, unparsed slices of the source; see the [module
/// documentation](self) for why.
#[derive(Debug, PartialEq, Clone, Copy)]
pub(super) enum LineKind<'a> {
    /// `:= <src>` - starts a node named `<src>`.
    NodeStart(Spanned<&'a str>),
    /// `---` - ends the current node.
    NodeEnd,
    /// `<speaker>: <text>` - a line of dialogue.
    Say {
        /// The speaker, or `None`.
        speaker: Option<Spanned<&'a str>>,
        /// What is being said.
        text: Spanned<&'a str>,
    },
    /// `-> <src>` - a choice offered to the player.
    Choice(Spanned<&'a str>),
    /// `=> <src>` - a jump to the node named `<src>`.
    Jump(Spanned<&'a str>),
    /// `>> <src>` - a command to execute on the engine.
    Command(Spanned<&'a str>),
    /// `[let <src>]` - assigns a variable.
    Let(Spanned<&'a str>),
    /// `[if <src>]` - opens a conditional block.
    If(Spanned<&'a str>),
    /// `[elif <src>]` - another condition of the block before it.
    Elif(Spanned<&'a str>),
    /// `[else]` - what the block before it does when no condition holds.
    /// Keeps its payload, which must be empty, so that `[else oups]` can be
    /// reported.
    Else(Spanned<&'a str>),
    /// A line matching no known marker. Holds the whole line so that the error
    /// can be reported later without losing what the author actually wrote.
    Malformed(Spanned<&'a str>),
}

/// A single source line, classified but not yet parsed.
#[derive(Debug, Clone, Copy)]
pub(super) struct RawLine<'a> {
    /// Content (trimmed, no indentation, no comments).
    #[allow(unused)]
    pub text: &'a str,
    /// Kind of the line and the content that follows its marker.
    pub kind: LineKind<'a>,
    /// Absolute position of `text` in the source, in bytes.
    pub span: Span,
    /// Indentation level.
    pub indent: u16,
}

pub(super) fn split_lines<'a>(src: &'a str, diagnostics: &mut Diagnostics) -> Vec<RawLine<'a>> {
    let mut out = Vec::new();
    let mut offset = 0usize;

    for raw in src.split_inclusive('\n') {
        // Compute the indent
        let body = raw.trim_start();
        let indent_length = raw.len() - body.len();
        if !body.is_empty() {
            if raw[..indent_length].contains('\t') {
                diagnostics.push(
                    Span::from_length(offset, indent_length),
                    DiagnosticKind::TabIndentation,
                );
            } else if indent_length.rem_euclid(4) > 0 {
                diagnostics.push(
                    Span::from_length(offset, indent_length),
                    DiagnosticKind::IrregularIndentation,
                );
            }
        }
        let indent = (indent_length / 4) as u16;

        // Remove comments
        let body = match body.split_once("//") {
            Some((s, _)) => s.trim_end(),
            None => body.trim_end(),
        };

        if !body.is_empty() {
            out.push(RawLine {
                text: body,
                kind: classify(body, offset + indent_length),
                span: Span::from_length(offset + indent_length, body.len()),
                indent,
            });
        }

        offset += raw.len();
    }

    out
}

pub fn classify(text: &str, offset: usize) -> LineKind<'_> {
    if text == "---" {
        return LineKind::NodeEnd;
    }
    if let Some(rest) = strip_marker(text, ":=", offset) {
        return LineKind::NodeStart(rest);
    }
    if let Some(rest) = strip_marker(text, "=>", offset) {
        return LineKind::Jump(rest);
    }
    if let Some(rest) = strip_marker(text, "->", offset) {
        return LineKind::Choice(rest);
    }
    if let Some(rest) = strip_marker(text, ">>", offset) {
        return LineKind::Command(rest);
    }
    if let Some(rest) = strip_bracket(text, "[let", offset) {
        return LineKind::Let(rest);
    }
    if let Some(rest) = strip_bracket(text, "[if", offset) {
        return LineKind::If(rest);
    }
    if let Some(rest) = strip_bracket(text, "[elif", offset) {
        return LineKind::Elif(rest);
    }
    if let Some(rest) = strip_bracket(text, "[else", offset) {
        return LineKind::Else(rest);
    }
    if let Some(marker) = malformed_marker(text, offset) {
        return LineKind::Malformed(marker);
    }
    split_speaker(text, offset)
}

fn strip_marker<'a>(text: &'a str, marker: &str, offset: usize) -> Option<Spanned<&'a str>> {
    let rest = text.strip_prefix(marker)?;
    // Requires a blank or the end of the line after the marker
    if rest.is_empty() || rest.starts_with(char::is_whitespace) {
        Some(Spanned::from_text(rest, offset + marker.len()))
    } else {
        None
    }
}

/// A bracketed marker, when what follows it cannot continue a word.
///
/// Unlike the other markers, a blank is not required: `[let]` is an empty
/// assignment, reported as such, rather than a line of text that happens to
/// start with a bracket. `[letter` stays a line of text.
fn strip_bracket<'a>(text: &'a str, marker: &str, offset: usize) -> Option<Spanned<&'a str>> {
    let rest = text.strip_prefix(marker)?;
    (!rest.starts_with(|c: char| c.is_alphanumeric() || c == '_'))
        .then(|| Spanned::from_text(rest, offset + marker.len()))
}

const MARKER_CHARS: [char; 4] = ['-', '=', ':', '>'];

/// Catches deformed markers (`----`, `==>`, `:==`)
fn malformed_marker(text: &str, offset: usize) -> Option<Spanned<&str>> {
    let n = text
        .chars()
        .take_while(|c| MARKER_CHARS.contains(c))
        .count();

    (n >= 2).then(|| Spanned {
        value: &text[..n],
        span: Span::from_length(offset, n),
    })
}

fn split_speaker(text: &str, offset: usize) -> LineKind<'_> {
    if let Some((prefix, rest)) = text.split_once(':') {
        let speaker = prefix.trim();
        if !speaker.is_empty() {
            let speaker = Spanned::from_text(speaker, offset);
            let text = Spanned::from_text(rest, offset + prefix.len() + 1);
            return LineKind::Say {
                speaker: Some(speaker),
                text,
            };
        }
    }

    let text = Spanned::from_text(text, offset);
    LineKind::Say {
        speaker: None,
        text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{LineIndex, diagnostic::Diagnostic};

    fn split(src: &str) -> (Vec<RawLine<'_>>, Vec<Diagnostic>) {
        let mut diagnostics = Diagnostics::new(LineIndex::new(src));
        let lines = split_lines(src, &mut diagnostics);
        (lines, diagnostics.iter().cloned().collect::<Vec<_>>())
    }

    fn texts(src: &str) -> Vec<&str> {
        split(src).0.into_iter().map(|l| l.text).collect()
    }

    #[test]
    fn split_lines_keeps_the_content_of_every_line() {
        assert_eq!(
            texts(":= start\nAlice: Salut\n---"),
            [":= start", "Alice: Salut", "---"]
        );
    }

    #[test]
    fn split_lines_skips_blank_lines() {
        assert_eq!(
            texts("\nAlice: Salut\n\n   \n\t\nBob: Salut\n"),
            ["Alice: Salut", "Bob: Salut"]
        );
    }

    #[test]
    fn split_lines_trims_indentation_and_trailing_spaces() {
        assert_eq!(texts("    -> Un choix   "), ["-> Un choix"]);
    }

    #[test]
    fn split_lines_counts_one_indent_level_per_four_spaces() {
        let (lines, diagnostics) = split("Alice: a\n    -> b\n        Bob: c");

        assert_eq!(
            lines.iter().map(|l| l.indent).collect::<Vec<_>>(),
            [0, 1, 2]
        );
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn split_lines_strips_comments() {
        assert_eq!(texts("Alice: Salut // Comments"), ["Alice: Salut"]);
        assert_eq!(texts("Alice: Salut// Comments"), ["Alice: Salut"]);
        assert!(texts("    // Comments").is_empty());
    }

    #[test]
    fn split_lines_spans_point_at_the_trimmed_text() {
        let src = "Alice: a\n    -> b // c\n";
        let (lines, _) = split(src);

        for line in &lines {
            assert_eq!(&src[line.span.start..line.span.end], line.text);
        }
        assert_eq!(lines[1].span.start, 13);
        assert_eq!(lines[1].span.end, 17);
    }

    #[test]
    fn split_lines_handles_crlf_endings() {
        let src = "Alice: Salut\r\nBob: Salut\r\n";
        let (lines, _) = split(src);

        assert_eq!(
            lines.iter().map(|l| l.text).collect::<Vec<_>>(),
            ["Alice: Salut", "Bob: Salut"]
        );
        assert_eq!(&src[lines[0].span.start..lines[0].span.end], "Alice: Salut");
    }

    #[test]
    fn error_on_tab_indentation() {
        let (lines, diagnostics) = split("\t-> Un choix");

        assert_eq!(lines.len(), 1);
        assert_eq!(diagnostics.len(), 1);
        assert!(matches!(
            diagnostics[0].kind,
            DiagnosticKind::TabIndentation
        ));
        assert_eq!(diagnostics[0].span.start, 0);
        assert_eq!(diagnostics[0].span.end, 1);
    }

    #[test]
    fn error_on_irregular_indentation() {
        let (_, diagnostics) = split("  -> Un choix");

        assert_eq!(diagnostics.len(), 1);
        assert!(matches!(
            diagnostics[0].kind,
            DiagnosticKind::IrregularIndentation
        ));
        assert_eq!(diagnostics[0].span.start, 0);
        assert_eq!(diagnostics[0].span.end, 2);
    }

    #[test]
    fn split_lines_does_not_report_indentation_on_blank_lines() {
        let (_, diagnostics) = split("Alice: a\n\n   \nBob: b\n");

        assert!(diagnostics.is_empty());
    }

    #[test]
    fn classify_detects_the_end_marker() {
        assert_eq!(classify("---", 0), LineKind::NodeEnd);
    }

    #[test]
    fn classify_detects_a_node_start() {
        assert_eq!(
            classify(":= start", 0),
            LineKind::NodeStart(Spanned::from_text("start", 3))
        );
        assert_eq!(
            classify(":=", 0),
            LineKind::NodeStart(Spanned::from_text("", 2))
        );
    }

    #[test]
    fn classify_detects_a_jump() {
        assert_eq!(
            classify("=> meeting", 0),
            LineKind::Jump(Spanned::from_text("meeting", 3))
        );
    }

    #[test]
    fn classify_detects_a_choice() {
        assert_eq!(
            classify("-> Un choix", 0),
            LineKind::Choice(Spanned::from_text("Un choix", 3))
        );
    }

    #[test]
    fn classify_detects_a_command() {
        assert_eq!(
            classify(">> command(arg1, arg2)", 0),
            LineKind::Command(Spanned::from_text("command(arg1, arg2)", 3))
        );
    }

    #[test]
    fn classify_detects_a_line_with_a_speaker() {
        assert_eq!(
            classify("Alice: Salut !", 0),
            LineKind::Say {
                speaker: Some(Spanned::from_text("Alice", 0)),
                text: Spanned::from_text("Salut !", 7)
            }
        );
    }

    #[test]
    fn classify_splits_a_line_on_its_first_colon_only() {
        assert_eq!(
            classify("Alice: Il est 12:30", 0),
            LineKind::Say {
                speaker: Some(Spanned::from_text("Alice", 0)),
                text: Spanned::from_text("Il est 12:30", 7)
            }
        );
    }

    #[test]
    fn classify_detects_a_line_without_a_speaker() {
        assert_eq!(
            classify("Salut !", 0),
            LineKind::Say {
                speaker: None,
                text: Spanned::from_text("Salut !", 0)
            }
        );
    }

    #[test]
    fn classify_detects_an_assignment() {
        assert_eq!(
            classify("[let $gold = 10]", 0),
            LineKind::Let(Spanned::from_text(" $gold = 10]", 4))
        );
    }

    /// The closing `]` belongs to the payload: this pass says what a line is,
    /// and `ast` is what reads and checks what it holds.
    #[test]
    fn classify_keeps_an_assignment_the_line_never_closes() {
        assert_eq!(
            classify("[let $gold = 10", 0),
            LineKind::Let(Spanned::from_text(" $gold = 10", 4))
        );
    }

    /// `[let` opens an assignment without a blank after it, unlike the other
    /// markers, but only when what follows cannot continue a word.
    #[test]
    fn classify_tells_an_assignment_from_a_word_starting_with_let() {
        assert_eq!(
            classify("[let]", 0),
            LineKind::Let(Spanned::from_text("]", 4))
        );
        assert_eq!(
            classify("[letter", 0),
            LineKind::Say {
                speaker: None,
                text: Spanned::from_text("[letter", 0)
            }
        );
    }

    #[test]
    fn classify_detects_the_branches_of_a_condition() {
        assert_eq!(
            classify("[if $gold > 5]", 0),
            LineKind::If(Spanned::from_text(" $gold > 5]", 3))
        );
        assert_eq!(
            classify("[elif $gold > 1]", 0),
            LineKind::Elif(Spanned::from_text(" $gold > 1]", 5))
        );
        assert_eq!(
            classify("[else]", 0),
            LineKind::Else(Spanned::from_text("]", 5))
        );
    }

    /// `[elif` and `[else` share their first two letters with nothing else,
    /// but `[if` is a prefix of no marker and `[iffy` is a line of text.
    #[test]
    fn classify_tells_a_branch_from_a_word_starting_with_it() {
        assert_eq!(
            classify("[iffy", 0),
            LineKind::Say {
                speaker: None,
                text: Spanned::from_text("[iffy", 0)
            }
        );
    }

    #[test]
    fn classify_requires_a_blank_after_a_marker() {
        assert_eq!(
            classify(":=start", 0),
            LineKind::Malformed(Spanned::from_text(":=", 0))
        );
        assert_eq!(
            classify("->>a", 0),
            LineKind::Malformed(Spanned::from_text("->>", 0))
        );
    }

    #[test]
    fn classify_detects_malformed_markers() {
        assert_eq!(
            classify("----", 0),
            LineKind::Malformed(Spanned::from_text("----", 0))
        );
        assert_eq!(
            classify("==> start", 0),
            LineKind::Malformed(Spanned::from_text("==>", 0))
        );
        assert_eq!(
            classify(":== start", 0),
            LineKind::Malformed(Spanned::from_text(":==", 0))
        );
    }
}
