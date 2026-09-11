//! Snapshots of the AST and the diagnostics for every file of `tests/corpus`.
//!
//! Spans are rendered as `line:col` plus the source slice they point at, so a
//! snapshot stays readable and does not churn when a byte offset shifts.

use replique::parser::{
    LineIndex, Parsed, Span,
    ast::{Choice, NodeDecl, Stmt, StmtKind},
    diagnostic::Diagnostic,
    parse,
};
use std::fmt::Write;

struct Src<'a> {
    raw: &'a str,
    index: LineIndex,
}

impl<'a> Src<'a> {
    fn from_str(raw: &'a str) -> Src<'a> {
        Self {
            raw,
            index: LineIndex::new(raw),
        }
    }
}

fn render(src: &Src, parsed: &Parsed) -> String {
    let mut out = String::new();

    out.push_str("== nodes ==\n");
    if parsed.nodes.is_empty() {
        out.push_str("(none)\n");
    }
    for node in &parsed.nodes {
        render_node(&mut out, src, node);
    }

    out.push_str("\n== diagnostics ==\n");
    if parsed.diagnostics.is_empty() {
        out.push_str("(none)\n");
    }
    for diag in parsed.diagnostics.iter() {
        render_diagnostic(&mut out, src, diag);
    }

    out
}

fn render_node(out: &mut String, src: &Src, node: &NodeDecl) {
    let _ = writeln!(out, "node {}", node.name.value);
    render_stmts(out, src, &node.body, 1);
}

fn render_stmts(out: &mut String, src: &Src, stmts: &[Stmt], depth: usize) {
    for stmt in stmts {
        let pad = "  ".repeat(depth);
        match &stmt.kind {
            StmtKind::Say { speaker, text } => {
                let speaker = speaker.as_ref().map_or("-", |s| &s.value);
                let _ = writeln!(
                    out,
                    "{pad}line {} {:?} {}",
                    speaker,
                    text.value,
                    at(src, stmt.span)
                );
            }
            StmtKind::Jump(target) => {
                let _ = writeln!(out, "{pad}jump {} {}", target.value, at(src, stmt.span));
            }
            StmtKind::Command { name, args } => {
                // Arguments are shown as the text they cover: an expression
                // debug-prints its own spans, which the doc above forbids.
                let args = args
                    .iter()
                    .map(|arg| &src.raw[arg.span.start..arg.span.end])
                    .collect::<Vec<_>>();
                let _ = writeln!(
                    out,
                    "{pad}command {} {:?} {}",
                    name.value,
                    args,
                    at(src, stmt.span)
                );
            }
            StmtKind::Set { name, value } => {
                let _ = writeln!(
                    out,
                    "{pad}set {} {:?} {}",
                    name.value,
                    &src.raw[value.span.start..value.span.end],
                    at(src, stmt.span)
                );
            }
            StmtKind::Choice { choices } => {
                let _ = writeln!(out, "{pad}choice-group {}", at(src, stmt.span));
                for Choice { text, body, span } in choices {
                    let _ = writeln!(out, "{pad}  choice {:?} {}", text.value, at(src, *span));
                    render_stmts(out, src, body, depth + 2);
                }
            }
        }
    }
}

fn render_diagnostic(out: &mut String, src: &Src, diag: &Diagnostic) {
    let _ = writeln!(
        out,
        "{} {} {}",
        diag.severity(),
        diag.kind.code(),
        at(src, diag.span)
    );
    for label in &diag.labels {
        let _ = writeln!(out, "  label {} {}", label.message, at(src, label.span));
    }
}

/// `line:col` of the span start, followed by the text it covers.
fn at(src: &Src, span: Span) -> String {
    let (line, col) = src.index.line_col(span.start);
    format!(
        "@{}:{} {:?}",
        line + 1,
        col + 1,
        &src.raw[span.start..span.end]
    )
}

macro_rules! corpus {
    ($($name:ident),* $(,)?) => {
        $(
            #[test]
            fn $name() {
                let src = Src::from_str(include_str!(concat!("corpus/", stringify!($name), ".rep")));
                let parsed = parse(src.raw);
                insta::assert_snapshot!(render(&src, &parsed));
            }
        )*
    };
}

corpus!(
    // Valid corpus
    minimal,
    example,
    nested_choices,
    empty_body,
    two_groups,
    unicode,
    forward_jump,
    jump_to_end,
    commands,
    lets,
    // Degraded corpus
    unclosed,
    unclosed_eof,
    stray_end,
    outside,
    bad_markers,
    bad_indent,
    end_in_choice,
    empty_choice,
    empty_node,
    bad_names,
    duplicate_nodes,
    unknown_jump,
    jump_in_choice,
    bad_commands,
    bad_lets,
);
