//! Syntax tree produce by the parser

use std::{collections::HashMap, iter::Peekable, vec::IntoIter};

use crate::parser::{
    END_NODE_NAME, Parsed, RESERVED_NODE_NAMES, Span, Spanned,
    diagnostic::{DiagnosticKind, Diagnostics, Label},
    expr::{Expr, pratt::ExprParser},
    lines::{LineKind, RawLine, split_lines},
};

/// Represents a dialogue node in the syntax tree
#[derive(Debug)]
pub struct NodeDecl {
    /// Name of the dialogue Node
    pub name: Spanned<String>,
    /// All the statement of the node
    pub body: Vec<Stmt>,
    /// Span of the full node. from `:= start` to `---`
    pub full_span: Span,
}

#[derive(Debug)]
pub enum StmtKind {
    Say {
        speaker: Option<Spanned<String>>,
        text: Spanned<String>,
    },
    Choice {
        choices: Vec<Choice>,
    },
    Jump(Spanned<String>),
    Command {
        name: Spanned<String>,
        args: Vec<Spanned<Expr>>,
    },
}

#[derive(Debug)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
}

#[derive(Debug)]
pub struct Choice {
    /// Text of the choice after `->`
    pub text: Spanned<String>,
    /// List of all Stmt in the body
    pub body: Vec<Stmt>,
    /// From the `->` to the end of its body.
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Bool(bool),
    String(String),
    Float(f64),
    Int(i64),
}

impl Value {
    pub fn parse(arg: &str) -> Option<Self> {
        match arg {
            "true" => return Some(Value::Bool(true)),
            "false" => return Some(Value::Bool(false)),
            _ => (),
        }

        // Guarded so that `inf` and `NaN`, which `f64` does parse, stay words.
        if arg.starts_with(|c: char| c.is_ascii_digit() || matches!(c, '-' | '+' | '.')) {
            if !arg.contains(['.', 'e', 'E'])
                && let Ok(int) = arg.parse()
            {
                return Some(Value::Int(int));
            }
            return arg.parse().ok().map(Value::Float);
        }

        if let Some(inner) = arg.strip_prefix('"').and_then(|a| a.strip_suffix('"')) {
            return Value::unescape(inner).map(Value::String);
        }

        is_valid_ident(arg).then(|| Value::String(arg.to_owned()))
    }

    /// Turns `\\` and `\"` back into the character they stand for. A trailing
    /// backslash, or an unescaped `"` closing the string early, is rejected.
    fn unescape(inner: &str) -> Option<String> {
        let mut out = String::with_capacity(inner.len());
        let mut chars = inner.chars();

        while let Some(c) = chars.next() {
            match c {
                '\\' => out.push(chars.next()?),
                '"' => return None,
                _ => out.push(c),
            }
        }

        Some(out)
    }
}

pub fn parse(src: &str) -> Parsed {
    let mut diagnostics = Diagnostics::from_src(src);
    let lines = split_lines(src, &mut diagnostics);

    let mut parser = Parser::new(lines, diagnostics);
    let nodes = parser.parse_file();

    Parsed {
        nodes,
        diagnostics: parser.diags,
    }
}

struct Parser<'a> {
    lines: Peekable<IntoIter<RawLine<'a>>>,
    diags: Diagnostics,
}

impl<'a> Parser<'a> {
    fn new(lines: Vec<RawLine<'a>>, diags: Diagnostics) -> Self {
        Self {
            lines: lines.into_iter().peekable(),
            diags,
        }
    }

    fn peek(&mut self) -> Option<RawLine<'a>> {
        self.lines.peek().copied()
    }

    fn bump(&mut self) -> Option<RawLine<'a>> {
        self.lines.next()
    }

    fn parse_file(&mut self) -> Vec<NodeDecl> {
        let mut nodes = vec![];

        while let Some(line) = self.peek() {
            match line.kind {
                LineKind::NodeStart(_) => nodes.push(self.parse_node()),
                LineKind::NodeEnd => {
                    self.diags.push(line.span, DiagnosticKind::StrayNodeEnd);
                    self.bump();
                }
                _ => {
                    self.diags
                        .push(line.span, DiagnosticKind::ContentOutsideNode);
                    self.skip_to_node_boundary();
                }
            }
        }

        self.validate_nodes(&nodes);

        nodes
    }

    fn skip_to_node_boundary(&mut self) {
        self.bump();
        while let Some(line) = self.peek() {
            if matches!(line.kind, LineKind::NodeStart(_) | LineKind::NodeEnd) {
                break;
            }
            self.bump();
        }
    }

    /// Parse a node from `:=` to `---` (or another node start or EOF)
    fn parse_node(&mut self) -> NodeDecl {
        let header = self.bump().expect("Already checked by peek.");
        let LineKind::NodeStart(name) = header.kind else {
            unreachable!("parse_node only start by NodeStart");
        };

        if header.indent > 0 {
            self.diags
                .push(header.span, DiagnosticKind::IndentedNodeStart);
        }

        // Validate node name
        if name.value.is_empty() {
            self.diags.push(name.span, DiagnosticKind::EmptyNodeName);
        } else if !is_valid_ident(name.value) {
            self.diags.push(
                name.span,
                DiagnosticKind::InvalidNodeName(name.value.into()),
            );
        }

        let body = self.parse_block(0);

        let end_span = match self.peek() {
            Some(RawLine {
                kind: LineKind::NodeEnd,
                span,
                indent,
                ..
            }) => {
                if indent > 0 {
                    self.diags.push(span, DiagnosticKind::IndentedNodeEnd);
                }
                self.bump();
                Some(span)
            }
            Some(RawLine {
                kind: LineKind::NodeStart(next),
                ..
            }) => {
                self.diags.push_labeled(
                    header.span,
                    DiagnosticKind::UnclosedNode(name.value.into()),
                    vec![Label {
                        span: next.span,
                        message: "the next node starts here".into(),
                    }],
                );
                None
            }
            None => {
                self.diags
                    .push(header.span, DiagnosticKind::UnclosedNode(name.value.into()));
                None
            }
            _ => unreachable!("parse_block only stops on NodeStart, NodeEnd or EOF"),
        };

        if body.is_empty() {
            self.diags.push(header.span, DiagnosticKind::EmptyNode);
        }

        // end_span is still None after this only if body is empty
        let last = end_span.or_else(|| body.last().map(|n| n.span));
        let full_span = last.map_or(header.span, |s| header.span.join(s));

        NodeDecl {
            name: name.into(),
            body,
            full_span,
        }
    }

    /// Parse the statements until a dedent below `min_indent` or a node boundary (':=', '---' or EOF)
    fn parse_block(&mut self, min_indent: u16) -> Vec<Stmt> {
        let mut out = vec![];

        while let Some(line) = self.peek() {
            // dedent below `min_indent`
            if line.indent < min_indent {
                break;
            }

            if matches!(line.kind, LineKind::NodeStart(_) | LineKind::NodeEnd) {
                break;
            }

            // The statement is more indented than what we expect.
            if line.indent > min_indent {
                self.diags
                    .push(line.span, DiagnosticKind::UnexpectedIndentation);
            }

            match line.kind {
                LineKind::Choice(_) => out.push(self.parse_choice_group()),
                _ => out.extend(self.parse_line()),
            }
        }

        out
    }

    /// Groups the consecutive `->` at the same indentation into a single
    /// [`StmtKind::Choice`], each option owning the block indented under it.
    fn parse_choice_group(&mut self) -> Stmt {
        let first = self.peek().expect("Already checked by parse_block.");
        let base = first.indent;
        let mut choices = vec![];

        while let Some(line) = self.peek() {
            // Anything else, or a `->` of another block, ends the group.
            let LineKind::Choice(text) = line.kind else {
                break;
            };
            if line.indent != base {
                break;
            }
            self.bump();

            if text.value.is_empty() {
                self.diags.push(line.span, DiagnosticKind::EmptyChoiceText);
            }

            let body = self.parse_block(base + 1);
            let span = body.last().map_or(line.span, |s| line.span.join(s.span));
            choices.push(Choice {
                text: text.into(),
                body,
                span,
            });
        }

        debug_assert!(!choices.is_empty(), "parse_block peeked a `->` first");

        let span = choices
            .last()
            .map_or(first.span, |c| first.span.join(c.span));
        if choices.len() == 1 {
            self.diags.push(span, DiagnosticKind::SingleChoice);
        }

        Stmt {
            kind: StmtKind::Choice { choices },
            span,
        }
    }

    /// Parse a line that is not a block line (choice, while, if, etc.)
    fn parse_line(&mut self) -> Option<Stmt> {
        let line = self.bump().expect("Already checked by peek.");

        match line.kind {
            LineKind::Say { speaker, text } => Some(Stmt {
                kind: StmtKind::Say {
                    speaker: speaker.map(|s| s.into()),
                    text: text.into(),
                },
                span: line.span,
            }),
            LineKind::Jump(name) if is_valid_ident(name.value) => Some(Stmt {
                kind: StmtKind::Jump(name.into()),
                span: line.span,
            }),
            LineKind::Jump(name) => {
                if name.value.is_empty() {
                    self.diags.push(line.span, DiagnosticKind::EmptyJump);
                } else {
                    self.diags.push(
                        name.span,
                        DiagnosticKind::InvalidNodeName(name.value.into()),
                    );
                }
                None
            }
            LineKind::Command(cmd) => self.parse_command(cmd, line.span),
            LineKind::Malformed(marker) => {
                self.diags.push(
                    line.span,
                    DiagnosticKind::MalformedMarker(marker.value.into()),
                );
                None
            }
            LineKind::Choice(_) | LineKind::NodeStart(_) | LineKind::NodeEnd => {
                debug_assert!(false, "handle by parse_block");
                None
            }
        }
    }

    /// `>> name()` or `>> name(1, $gold + 1, true)`. The `(` follows the name
    /// directly, and every argument is an expression.
    /// A malformed command is dropped.
    fn parse_command(&mut self, cmd: Spanned<&str>, line_span: Span) -> Option<Stmt> {
        // The name is read before the expression parser sees the line: a
        // faulty one is named as such, instead of being reported as a whole
        // expression that happens not to be a call.
        let name = cmd
            .value
            .split('(')
            .next()
            .expect("always one part")
            .trim_end();
        if name.is_empty() {
            self.diags.push(line_span, DiagnosticKind::EmptyCommand);
            return None;
        }
        if !is_valid_ident(name) {
            self.diags.push(
                Span::from_length(cmd.span.start, name.len()),
                DiagnosticKind::InvalidCommandName(name.to_owned()),
            );
            return None;
        }

        let expr = ExprParser::new(&cmd, &mut self.diags).expr(0);

        match expr.value {
            Expr::Function { name, args } => {
                if cmd.span != expr.span {
                    // Spaces between the `)` and the text are not the text.
                    let rest = cmd.value[expr.span.end - cmd.span.start..].trim_start();
                    self.diags.push(
                        Span::new(cmd.span.end - rest.len(), cmd.span.end),
                        DiagnosticKind::TrailingAfterCommand,
                    );
                }

                Some(Stmt {
                    kind: StmtKind::Command { name, args },
                    span: expr.span,
                })
            }
            Expr::Error => None,
            _ => {
                self.diags.push(expr.span, DiagnosticKind::ExpectedCommand);
                None
            }
        }
    }

    /// Detect duplicate node name and unkonwn jump node
    fn validate_nodes(&mut self, nodes: &Vec<NodeDecl>) {
        // Detect jump to uknown node
        let names = nodes.iter().map(|n| &n.name.value).collect::<Vec<_>>();
        for node in nodes {
            self.validate_jumps(&node.body, &names);
        }

        // Detect reserved node names
        let reserved = nodes
            .iter()
            .filter(|n| RESERVED_NODE_NAMES.contains(&n.name.value.as_str()));

        for node in reserved {
            self.diags.push(
                node.name.span,
                DiagnosticKind::ReservedNodeName(node.name.value.clone()),
            );
        }

        // Detect duplicate node names
        let dup = nodes
            .iter()
            .enumerate()
            .fold(
                HashMap::<&String, Vec<usize>>::new(),
                |mut dup, (i, node)| {
                    dup.entry(&node.name.value)
                        .and_modify(|e| e.push(i))
                        .or_insert(vec![i]);
                    dup
                },
            )
            .into_iter()
            .filter(|(_, v)| v.len() > 1)
            .collect::<HashMap<&String, Vec<usize>>>();

        for (name, indices) in dup {
            for i in indices {
                self.diags.push(
                    nodes[i].name.span,
                    DiagnosticKind::DuplicateNodeFound(name.clone()),
                );
            }
        }
    }

    /// Walks choice bodies too: a `=>` most often sits inside an option.
    fn validate_jumps(&mut self, stmts: &[Stmt], names: &[&String]) {
        for stmt in stmts {
            match &stmt.kind {
                StmtKind::Jump(name)
                    if !names.contains(&&name.value) && name.value != END_NODE_NAME =>
                {
                    self.diags.push(
                        stmt.span,
                        DiagnosticKind::JumpToUnknownNode(name.value.clone()),
                    );
                }
                StmtKind::Choice { choices } => {
                    for choice in choices {
                        self.validate_jumps(&choice.body, names);
                    }
                }
                _ => (),
            }
        }
    }
}

pub(super) fn is_valid_ident(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{
        diagnostic::DiagnosticKind,
        expr::{BinaryOp, UnaryOp},
    };

    /// The single statement of a one-node file.
    fn only_stmt(src: &str) -> StmtKind {
        let mut parsed = parse(src);
        assert_eq!(parsed.nodes.len(), 1, "expected a single node");

        let mut node = parsed.nodes.pop().expect("checked above");
        assert_eq!(node.body.len(), 1, "expected a single statement");
        node.body.pop().expect("checked above").kind
    }

    /// Wraps `line` in a node, so that a statement can be tested on its own.
    fn in_node(line: &str) -> String {
        format!(":= start\n{line}\n---\n")
    }

    /// Same, followed by a valid line: a dropped statement would otherwise
    /// leave the node empty and add an `empty-node` warning to the codes.
    fn in_filled_node(line: &str) -> String {
        format!(":= start\n{line}\nAlice: ok\n---\n")
    }

    /// Name and arguments of a command, spans dropped.
    fn command(line: &str) -> (String, Vec<Expr>) {
        match only_stmt(&in_node(line)) {
            StmtKind::Command { name, args } => {
                (name.value, args.into_iter().map(|a| a.value).collect())
            }
            other => panic!("expected a command, got {other:?}"),
        }
    }

    /// A literal argument, as the expression parser builds it.
    fn lit(value: Value) -> Expr {
        Expr::Litteral { value }
    }

    fn codes(src: &str) -> Vec<&'static str> {
        parse(src)
            .diagnostics
            .iter()
            .map(|d| d.kind.code())
            .collect()
    }

    #[test]
    fn a_broken_node_does_not_contaminate_the_next_one() {
        let parsed = parse(":= a\n---- oups\n---\n:= b\nAlice: ok\n---\n");

        assert_eq!(parsed.nodes.len(), 2);
        assert_eq!(parsed.nodes[1].name.value, "b");
        assert_eq!(parsed.nodes[1].body.len(), 1);
    }

    #[test]
    fn an_unclosed_node_keeps_its_body_and_the_next_node_is_parsed() {
        let src = ":= a\nAlice: un\n:= b\nAlice: deux\n---\n";
        let parsed = parse(src);

        assert_eq!(parsed.nodes.len(), 2);
        assert_eq!(parsed.nodes[0].body.len(), 1);
        assert_eq!(parsed.nodes[1].body.len(), 1);
        assert_eq!(codes(src), ["unclosed-node"]);
    }

    #[test]
    fn a_whole_stray_region_costs_a_single_diagnostic() {
        assert_eq!(
            codes("Alice: a\nBob: b\nCharlie: c\n:= start\nAlice: d\n---\n"),
            ["content-outside-node"]
        );
    }

    #[test]
    fn a_deeper_choice_nests_into_the_body_of_the_previous_one() {
        let parsed = parse(":= start\n-> A\n    -> B\n---\n");
        let [step] = &parsed.nodes[0].body[..] else {
            panic!("expected a single choice group");
        };
        let StmtKind::Choice { choices } = &step.kind else {
            panic!("expected a choice group");
        };

        assert_eq!(choices.len(), 1);
        assert_eq!(choices[0].text.value, "A");
        assert!(matches!(
            choices[0].body[..],
            [Stmt {
                kind: StmtKind::Choice { .. },
                ..
            }]
        ));
    }

    #[test]
    fn a_node_is_kept_even_when_its_name_is_invalid() {
        let src = ":= 2fast\nAlice: ok\n---\n";
        let parsed = parse(src);

        assert_eq!(parsed.nodes.len(), 1);
        assert_eq!(parsed.nodes[0].name.value, "2fast");
        assert_eq!(codes(src), ["invalid-node-name"]);
    }

    /// An empty name is already reported: calling it invalid on top would
    /// print an empty name in backticks.
    #[test]
    fn an_empty_name_is_reported_once() {
        assert_eq!(codes(":= \nAlice: ok\n---\n"), ["empty-node-name"]);
        assert_eq!(codes(&in_filled_node(">> ()")), ["empty-command"]);
    }

    #[test]
    fn an_unusable_line_costs_exactly_one_line() {
        let parsed = parse(":= start\n=>\nAlice: ok\n---\n");

        assert_eq!(parsed.nodes[0].body.len(), 1);
        assert!(matches!(parsed.nodes[0].body[0].kind, StmtKind::Say { .. }));
        assert_eq!(
            parsed.diagnostics.iter().next().unwrap().kind,
            DiagnosticKind::EmptyJump
        );
    }

    #[test]
    fn the_full_span_covers_the_header_and_the_terminator() {
        let src = ":= start\nAlice: ok\n---\n";
        let node = &parse(src).nodes[0];

        assert_eq!(&src[node.name.span.start..node.name.span.end], "start");
        assert_eq!(
            &src[node.full_span.start..node.full_span.end],
            ":= start\nAlice: ok\n---"
        );
    }

    #[test]
    fn a_command_without_argument_keeps_its_parentheses() {
        assert_eq!(command(">> pause()"), ("pause".to_string(), vec![]));
        assert_eq!(codes(&in_filled_node(">> pause")), ["expected-command"]);
    }

    #[test]
    fn a_command_parses_every_kind_of_value() {
        let (name, args) = command(r#">> play("bell", 0.5, 2, true, loop)"#);

        assert_eq!(name, "play");
        assert_eq!(
            args,
            vec![
                lit(Value::String("bell".into())),
                lit(Value::Float(0.5)),
                lit(Value::Int(2)),
                lit(Value::Bool(true)),
                lit(Value::String("loop".into())),
            ]
        );
    }

    /// `-2` is the negation of `2`, not a literal: the lexer never gives a
    /// sign to a number.
    #[test]
    fn a_negative_number_is_a_negation() {
        let (_, args) = command(">> play(-2)");

        assert!(
            matches!(
                args.as_slice(),
                [Expr::Unary { op, rhs }]
                    if op.value == UnaryOp::Neg && rhs.value == lit(Value::Int(2))
            ),
            "expected a negation, got {args:?}"
        );
    }

    #[test]
    fn an_argument_can_be_a_whole_expression() {
        let (_, args) = command(">> play($gold + 1)");

        assert!(
            matches!(
                args.as_slice(),
                [Expr::Binary { op, lhs, rhs }]
                    if op.value == BinaryOp::Add
                        && lhs.value == (Expr::Var { name: "gold".into() })
                        && rhs.value == lit(Value::Int(1))
            ),
            "expected an addition, got {args:?}"
        );
    }

    #[test]
    fn a_comma_inside_a_string_does_not_split_the_arguments() {
        let (_, args) = command(r#">> say("un, deux")"#);

        assert_eq!(args, vec![lit(Value::String("un, deux".into()))]);
    }

    #[test]
    fn a_string_argument_unescapes_its_quotes_and_backslashes() {
        let (_, args) = command(r#">> say("a \" b \\ c")"#);

        assert_eq!(args, vec![lit(Value::String(r#"a " b \ c"#.into()))]);
    }

    #[test]
    fn spaces_around_the_arguments_are_ignored() {
        assert_eq!(
            command(">> play( 1 , 2 )"),
            (
                "play".to_string(),
                vec![lit(Value::Int(1)), lit(Value::Int(2))]
            )
        );
    }

    /// `play (1)` reads as the word `play` followed by a parenthesis, not as
    /// a call: the `(` belongs to the name.
    #[test]
    fn a_space_before_the_parenthesis_is_not_a_call() {
        assert_eq!(codes(&in_filled_node(">> play (1)")), ["expected-command"]);
    }

    #[test]
    fn a_word_that_a_float_would_parse_stays_a_string() {
        let (_, args) = command(">> play(inf, NaN)");

        assert_eq!(
            args,
            vec![
                lit(Value::String("inf".into())),
                lit(Value::String("NaN".into()))
            ]
        );
    }

    #[test]
    fn a_command_is_a_statement_of_its_block() {
        let parsed = parse(":= start\nAlice: a\n-> A\n    >> play(1)\n-> B\n    Alice: b\n---\n");
        let StmtKind::Choice { choices } = &parsed.nodes[0].body[1].kind else {
            panic!("expected a choice group");
        };

        assert!(matches!(choices[0].body[0].kind, StmtKind::Command { .. }));
    }

    #[test]
    fn error_on_a_command_without_a_name() {
        assert_eq!(codes(&in_filled_node(">>")), ["empty-command"]);
        assert_eq!(codes(&in_filled_node(">> ()")), ["empty-command"]);
    }

    #[test]
    fn error_on_an_invalid_command_name() {
        assert_eq!(
            codes(&in_filled_node(">> 2fast(1)")),
            ["invalid-command-name"]
        );
    }

    #[test]
    fn error_on_an_unclosed_argument_list() {
        assert_eq!(codes(&in_filled_node(">> play(1")), ["unclosed-call"]);
    }

    /// Text left after the `)` has its own diagnostic, so that the arguments
    /// before it are not blamed.
    #[test]
    fn error_on_a_trailing_token_after_a_command() {
        assert_eq!(
            codes(&in_filled_node(">> play(1) oups")),
            ["trailing-after-command"]
        );
    }

    /// A `)` inside an unterminated string is not the closing one.
    #[test]
    fn error_on_an_unterminated_string_argument() {
        assert_eq!(
            codes(&in_filled_node(r#">> play("oups)"#)),
            ["unterminated-string", "unclosed-call"]
        );
    }

    #[test]
    fn error_on_an_argument_that_does_not_parse() {
        assert_eq!(codes(&in_filled_node(">> play(1..2)")), ["invalid-number"]);
        assert_eq!(
            codes(&in_filled_node(">> play(1,)")),
            ["expected-expression"]
        );
        assert_eq!(
            codes(&in_filled_node(">> play(1 2)")),
            ["expected-arg-separator"]
        );
    }

    #[test]
    fn a_broken_command_costs_exactly_one_line() {
        let src = ":= start\n>> play(1 2)\nAlice: ok\n---\n";
        let parsed = parse(src);

        assert_eq!(parsed.nodes[0].body.len(), 1);
        assert!(matches!(parsed.nodes[0].body[0].kind, StmtKind::Say { .. }));
    }

    #[test]
    fn a_command_diagnostic_points_at_the_offending_argument() {
        let src = &in_filled_node(">> play(1, 1..2)");
        let parsed = parse(src);
        let diag = parsed.diagnostics.iter().next().unwrap();

        assert_eq!(&src[diag.span.start..diag.span.end], "1..2");
    }

    #[test]
    fn a_command_is_not_confused_with_a_malformed_marker() {
        assert_eq!(codes(&in_filled_node(">>>play")), ["malformed-marker"]);
    }
}
