//! Syntax tree produce by the parser

use std::{collections::HashMap, iter::Peekable, vec::IntoIter};

use crate::{
    builtins::lookup,
    parser::{
        AWAIT_KEYWORD, END_NODE_NAME, Parsed, RESERVED_COMMAND_NAMES, RESERVED_NODE_NAMES, Span,
        Spanned,
        diagnostic::{
            DiagnosticKind::{self, StrayLoopControl},
            Diagnostics, Label,
        },
        expr::{Expr, ValueType, pratt},
        lines::{LineKind, RawLine, split_lines},
        tags::{Tag, split_tags, tag_starts, unescape},
    },
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

impl NodeDecl {
    /// Every statement of the node. The blocks are flattened.
    pub fn all_statements(&self) -> Vec<&Stmt> {
        let mut all = vec![];
        flatten_stmts(&self.body, &mut all);
        all
    }

    /// Every call to a function written in the node, in the order they are
    /// written, a call coming before the ones in its arguments.
    ///
    /// Functions live in expressions, so this looks wherever one can be:
    /// the text of a line or of a choice, the arguments of a command, the
    /// value of a `[let]`, and the condition of an `[if]` or of a `[while]`.
    /// A `>>` command is not a function, and is not listed.
    pub fn all_calls(&self) -> Vec<Call<'_>> {
        let mut calls = vec![];

        for stmt in self.all_statements() {
            match &stmt.kind {
                StmtKind::Say { text, .. } => text_calls(text, &mut calls),
                StmtKind::Choice { choices } => {
                    for choice in choices {
                        text_calls(&choice.text, &mut calls);
                    }
                }
                StmtKind::Command { args, .. } => {
                    for arg in args {
                        expr_calls(&arg.value, arg.span, &mut calls);
                    }
                }
                StmtKind::Set { value, .. } => expr_calls(&value.value, value.span, &mut calls),
                StmtKind::If { branches, .. } => {
                    for branch in branches {
                        let condition = &branch.condition;
                        expr_calls(&condition.value, condition.span, &mut calls);
                    }
                }
                StmtKind::While { condition, .. } => {
                    expr_calls(&condition.value, condition.span, &mut calls);
                }
                StmtKind::Jump(_) | StmtKind::Break | StmtKind::Continue => {}
            }
        }

        calls
    }
}

/// A call to a function, as [`NodeDecl::all_calls`] finds it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Call<'a> {
    /// Name of the function. Its span takes the `(` along.
    pub name: &'a Spanned<String>,
    /// How many arguments the call gives.
    pub args: usize,
    /// The whole call, from its name to its `)`.
    pub span: Span,
}

#[derive(Debug)]
pub enum StmtKind {
    Say {
        speaker: Option<Spanned<String>>,
        text: Vec<Spanned<TextPart>>,
        /// The `#tags` written at the start and at the end of the text, in
        /// the order they are written.
        tags: Vec<Spanned<Tag>>,
    },
    Choice {
        choices: Vec<Choice>,
    },
    Jump(Spanned<String>),
    Command {
        name: Spanned<String>,
        args: Vec<Spanned<Expr>>,
        awaited: bool,
    },
    Set {
        /// Name of the variable, without its `$`.
        name: Spanned<String>,
        /// Name of the attributes of the dict
        attrs: Vec<Spanned<String>>,
        value: Spanned<Expr>,
    },
    /// `[if]`, its `[elif]` and its `[else]`, in the order they are written.
    If {
        /// The `[if]` and every `[elif]` after it, never empty.
        branches: Vec<Branch>,
        /// Body of the `[else]`, when the block has one.
        otherwise: Option<Vec<Stmt>>,
    },
    /// `[while]`
    While {
        condition: Spanned<Expr>,
        body: Vec<Stmt>,
    },
    Break,
    Continue,
}

/// One `[if <cond>]` or `[elif <cond>]`, and the block indented under it.
#[derive(Debug)]
pub struct Branch {
    pub condition: Spanned<Expr>,
    pub body: Vec<Stmt>,
    /// From the marker to the last statement of the body.
    pub span: Span,
}

#[derive(Debug)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
}

#[derive(Debug)]
pub struct Choice {
    /// Text of the choice after `->`, split into its literal and inline
    /// expression parts.
    pub text: Vec<Spanned<TextPart>>,
    /// The `#tags` written at the start and at the end of the text, in the
    /// order they are written.
    pub tags: Vec<Spanned<Tag>>,
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

#[derive(Debug)]
pub enum TextPart {
    Text(String),
    Expression(Expr),
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
    /// Whether the statements being read are under a `[while]`
    in_loop: bool,
}

impl<'a> Parser<'a> {
    fn new(lines: Vec<RawLine<'a>>, diags: Diagnostics) -> Self {
        Self {
            lines: lines.into_iter().peekable(),
            diags,
            in_loop: false,
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
                LineKind::If(_) => out.push(self.parse_if()),
                LineKind::While(_) => out.extend(self.parse_while()),
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

            // Checked once the tags are off: a choice made of tags alone
            // gives the player nothing to pick.
            let (text, tags) = self.split_tags(text);
            if text.value.is_empty() {
                self.diags.push(line.span, DiagnosticKind::EmptyChoiceText);
            }

            let body = self.parse_block(base + 1);
            let span = body.last().map_or(line.span, |s| line.span.join(s.span));
            choices.push(Choice {
                text: self.parse_text_line(text),
                tags,
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

    /// Groups an `[if]` with the `[elif]` and `[else]` that follow it at the
    /// same indentation, each owning the block indented under it.
    ///
    /// A branch that cannot be there — an `[elif]` after the `[else]`, a
    /// second `[else]` — is reported and dropped with its body, which nothing
    /// could have reached anyway.
    fn parse_if(&mut self) -> Stmt {
        let first = self.peek().expect("Already checked by parse_block.");
        let base = first.indent;
        let mut branches: Vec<Branch> = vec![];
        let mut otherwise = None;
        let mut span = first.span;
        // Whether the `[if]` has been read, and not whether a branch came out
        // of it: a condition that is dropped must not let the next `[if]`
        // join this group.
        let mut started = false;

        while let Some(line) = self.peek() {
            if line.indent != base {
                break;
            }

            // What this line is for the group: a condition to read, or the
            // `[else]`. Anything else, and the group is over.
            let marker = match line.kind {
                LineKind::If(_) if !started => "[if]",
                LineKind::Elif(_) if started => "[elif]",
                LineKind::Else(_) if started => "[else]",
                _ => break,
            };
            started = true;
            let kind = line.kind;
            let line_span = line.span;
            self.bump();

            let body = self.parse_block(base + 1);
            let body_span = body.last().map_or(line_span, |s| line_span.join(s.span));
            span = span.join(body_span);

            if otherwise.is_some() {
                self.diags
                    .push(line_span, DiagnosticKind::BranchAfterElse(marker.into()));
                continue;
            }

            match kind {
                LineKind::Else(rest) => {
                    self.expect_empty_bracket(rest, line_span, marker);
                    otherwise = Some(body);
                }
                LineKind::If(cond) | LineKind::Elif(cond) => {
                    match self.parse_condition(cond, line_span, marker) {
                        Some(condition) => branches.push(Branch {
                            condition,
                            body,
                            span: body_span,
                        }),
                        // The condition is already reported; keeping the
                        // branch would mean keeping a body nothing guards.
                        None => continue,
                    }
                }
                _ => unreachable!("checked when the marker was named"),
            }
        }

        // A group where every branch was dropped is empty, and compiles to
        // nothing. It only happens once something has been reported, so the
        // file has no dialogue to run anyway.
        Stmt {
            kind: StmtKind::If {
                branches,
                otherwise,
            },
            span,
        }
    }

    /// `[while <cond>]` and the block indented under it, which is the only
    /// place a `[break]` or a `[continue]` may be.
    ///
    /// A loop whose condition is dropped is dropped with its body, which
    /// nothing would have run.
    fn parse_while(&mut self) -> Option<Stmt> {
        let line = self.bump().expect("Already checked by parse_block.");
        let LineKind::While(cond) = line.kind else {
            unreachable!("parse_while only starts on a While");
        };

        // The body is read even when the condition is faulty, so that what it
        // holds is reported too, and so that the block is consumed either way.
        let prev = std::mem::replace(&mut self.in_loop, true);
        let body = self.parse_block(line.indent + 1);
        self.in_loop = prev;

        let span = body.last().map_or(line.span, |s| line.span.join(s.span));
        let condition = self.parse_condition(cond, line.span, "[while]")?;

        Some(Stmt {
            kind: StmtKind::While { condition, body },
            span,
        })
    }

    /// The condition of an `[if]` or an `[elif]`, which must be a `bool`.
    fn parse_condition(
        &mut self,
        body: Spanned<&str>,
        line_span: Span,
        marker: &str,
    ) -> Option<Spanned<Expr>> {
        let inner = self.strip_bracket(body, line_span, marker)?;
        let (expr, trailing) = pratt::parse_with_trailing(&inner, &mut self.diags);

        if let Some(span) = trailing {
            self.diags
                .push(span, DiagnosticKind::UnexpectedTextInBracket);
            return None;
        }
        if let Expr::Error = expr.value {
            return None;
        }

        // `Unknown` is what a variable or a call is worth before the dialogue
        // runs: the VM checks those again when it evaluates them.
        match expr.value.value_type() {
            Some(ValueType::Bool | ValueType::Unknown) => Some(expr),
            Some(other) => {
                self.diags.push(
                    expr.span,
                    DiagnosticKind::ConditionIsNotABool(other.to_string()),
                );
                None
            }
            // An operator applied to the wrong types, already reported.
            None => None,
        }
    }

    /// Text between a bracketed marker and its `]`.
    fn strip_bracket<'b>(
        &mut self,
        body: Spanned<&'b str>,
        line_span: Span,
        marker: &str,
    ) -> Option<Spanned<&'b str>> {
        match body.value.trim_end().strip_suffix(']') {
            Some(inner) => Some(Spanned::from_text(inner.trim_end(), body.span.start)),
            None => {
                self.diags.push(
                    line_span,
                    DiagnosticKind::UnclosedBracket(marker.trim_end_matches(']').into()),
                );
                None
            }
        }
    }

    /// A marker that takes nothing, such as `[else]` or `[break]`.
    fn expect_empty_bracket(&mut self, body: Spanned<&str>, line_span: Span, marker: &str) {
        let Some(inner) = self.strip_bracket(body, line_span, marker) else {
            return;
        };
        if !inner.value.trim().is_empty() {
            self.diags
                .push(inner.span, DiagnosticKind::UnexpectedTextInBracket);
        }
    }

    /// Parse a line that is not a block line (choice, while, if, etc.)
    fn parse_line(&mut self) -> Option<Stmt> {
        let line = self.bump().expect("Already checked by peek.");

        match line.kind {
            LineKind::Say { speaker, text } => {
                let (text, tags) = self.split_tags(text);
                Some(Stmt {
                    kind: StmtKind::Say {
                        speaker: speaker.map(|s| s.into()),
                        text: self.parse_text_line(text),
                        tags,
                    },
                    span: line.span,
                })
            }
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
            LineKind::Let(body) => self.parse_let(body, line.span),
            // The block under a branch with no `[if]` has nowhere to go either
            LineKind::Elif(_) | LineKind::Else(_) => {
                let marker = match line.kind {
                    LineKind::Elif(_) => "[elif]",
                    _ => "[else]",
                };
                self.diags
                    .push(line.span, DiagnosticKind::StrayBranch(marker.into()));
                self.parse_block(line.indent + 1);
                None
            }
            LineKind::Break(s) | LineKind::Continue(s) => {
                let marker = match line.kind {
                    LineKind::Break(_) => "[break]",
                    _ => "[continue]",
                };

                // Not in while loop
                if !self.in_loop {
                    self.diags.push(line.span, StrayLoopControl(marker.into()));
                    return None;
                }

                // report diagnostic from malformed braket `[break` or `[continue $name]`
                self.expect_empty_bracket(s, line.span, marker);
                Some(Stmt {
                    kind: match line.kind {
                        LineKind::Break(_) => StmtKind::Break,
                        _ => StmtKind::Continue,
                    },
                    span: line.span,
                })
            }
            LineKind::Malformed(marker) => {
                self.diags.push(
                    line.span,
                    DiagnosticKind::MalformedMarker(marker.value.into()),
                );
                None
            }
            LineKind::If(_)
            | LineKind::While(_)
            | LineKind::Choice(_)
            | LineKind::NodeStart(_)
            | LineKind::NodeEnd => {
                debug_assert!(false, "handle by parse_block");
                None
            }
        }
    }

    /// `>> name()` or `>> name(1, $gold + 1, true)`, and `>> await name()`
    fn parse_command(&mut self, mut cmd: Spanned<&str>, line_span: Span) -> Option<Stmt> {
        // `await` is a word of the language, never a name: what follows it is
        // read as the command, and `>> await()` is refused below.
        let awaited = matches!(
            cmd.value.strip_prefix(AWAIT_KEYWORD),
            Some(rest) if rest.starts_with(char::is_whitespace)
        );

        if awaited {
            let rest = &cmd.value[AWAIT_KEYWORD.len()..];
            cmd = Spanned::from_text(rest, cmd.span.start + AWAIT_KEYWORD.len());
        }

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

        if RESERVED_COMMAND_NAMES.contains(&name) {
            self.diags.push(
                Span::from_length(cmd.span.start, name.len()),
                DiagnosticKind::ReservedCommandName(name.to_owned()),
            );
            return None;
        }

        let expr = pratt::parse(&cmd, &mut self.diags);

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
                    kind: StmtKind::Command {
                        name,
                        args,
                        awaited,
                    },
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

    /// `[let $name = <expr>]`. A malformed assignment is dropped.
    fn parse_let(&mut self, body: Spanned<&str>, line_span: Span) -> Option<Stmt> {
        let inner = self.strip_bracket(body, line_span, "[let")?;

        let Some(assignment) = pratt::parse_assignment(&inner, &mut self.diags) else {
            self.diags
                .push(line_span, DiagnosticKind::ExpectedAssignment);
            return None;
        };

        if let Some(span) = assignment.trailing {
            self.diags
                .push(span, DiagnosticKind::UnexpectedTextInBracket);
            return None;
        }
        if let Expr::Error = assignment.value.value {
            return None;
        }

        Some(Stmt {
            kind: StmtKind::Set {
                name: assignment.name,
                attrs: assignment.attrs,
                value: assignment.value,
            },
            span: line_span,
        })
    }

    /// Detect duplicate node name and unkonwn jump node
    fn validate_nodes(&mut self, nodes: &Vec<NodeDecl>) {
        // Detect jump to uknown node
        let names = nodes.iter().map(|n| &n.name.value).collect::<Vec<_>>();
        for node in nodes {
            self.validate_jumps(node, &names);
            self.validate_builtins(node);
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

    /// Reports every call to a builtin that does not give it the number of
    /// arguments it takes.
    ///
    /// Only the builtins are checked here: they are the functions every
    /// dialogue has, and the host cannot answer in their place. What the game
    /// adds is the business of its schema.
    fn validate_builtins(&mut self, node: &NodeDecl) {
        for call in node.all_calls() {
            if let Some(builtin) = lookup(&call.name.value)
                && !builtin.arity.accepts(call.args)
            {
                self.diags.push(
                    call.span,
                    DiagnosticKind::BuiltinArity {
                        name: call.name.value.clone(),
                        got: call.args,
                        expected: builtin.arity,
                    },
                );
            }
        }
    }

    /// Reports every `=>` of `node` that leads nowhere.
    fn validate_jumps(&mut self, node: &NodeDecl, names: &[&String]) {
        for stmt in node.all_statements() {
            if let StmtKind::Jump(name) = &stmt.kind
                && !names.contains(&&name.value)
                && name.value != END_NODE_NAME
            {
                self.diags.push(
                    stmt.span,
                    DiagnosticKind::JumpToUnknownNode(name.value.clone()),
                );
            }
        }
    }

    /// Splits `Some text from [$var]` into the parts it is made of.
    ///
    /// A `[` opens an inline expression that the next `]` closes; everything
    /// else is text. What the expression parser does not read inside the
    /// brackets is reported here, where the bracket is known, exactly as a
    /// `[let]` or an `[if]` reports its own trailing text.
    ///
    /// A `[` the line never closes is reported and kept as text, so the line
    /// still says something instead of losing its end.
    fn parse_text_line(&mut self, text: Spanned<&str>) -> Vec<Spanned<TextPart>> {
        let Spanned { value: text, span } = text;
        let mut parts = Vec::new();
        let mut rest = 0;

        while let Some(open) = text[rest..].find('[') {
            let open = rest + open;
            self.report_misplaced_tags(text, span, rest..open);
            push_text(&mut parts, text, span, rest..open);

            let Some(close) = text[open..].find(']') else {
                self.diags.push(
                    Span::from_length(span.start + open, text.len() - open),
                    DiagnosticKind::UnclosedBracket("[".to_owned()),
                );
                push_text(&mut parts, text, span, open..text.len());
                return parts;
            };
            let close = open + close;

            let inner_span = Span::from_length(span.start + open + 1, close - open - 1);
            let inner = Spanned::new(&text[open + 1..close], inner_span);
            let (expr, trailing) = pratt::parse_with_trailing(&inner, &mut self.diags);
            if let Some(span) = trailing {
                self.diags
                    .push(span, DiagnosticKind::UnexpectedTextInBracket);
            }
            parts.push(Spanned::new(TextPart::Expression(expr.value), inner_span));

            rest = close + 1;
        }

        self.report_misplaced_tags(text, span, rest..text.len());
        push_text(&mut parts, text, span, rest..text.len());
        parts
    }

    /// Takes the tags off both ends of `text`, reporting a name given twice.
    fn split_tags<'t>(&mut self, text: Spanned<&'t str>) -> (Spanned<&'t str>, Vec<Spanned<Tag>>) {
        let (text, tags) = split_tags(text);

        for (at, tag) in tags.iter().enumerate() {
            if let Some(first) = tags[..at].iter().find(|t| t.value.name == tag.value.name) {
                self.diags.push_labeled(
                    tag.span,
                    DiagnosticKind::DuplicateTag(tag.value.name.clone()),
                    vec![Label {
                        span: first.span,
                        message: "first given here".to_owned(),
                    }],
                );
            }
        }

        (text, tags)
    }

    /// Warns about what opens like a tag in `text[range]`. The tags that are
    /// read are already gone by then, so anything left is in the middle of
    /// the line, or is not shaped like a tag, and stays in the text.
    fn report_misplaced_tags(&mut self, text: &str, span: Span, range: std::ops::Range<usize>) {
        for word in tag_starts(text, range) {
            self.diags.push(
                Span::from_length(span.start + word.start, word.len()),
                DiagnosticKind::MisplacedTag(text[word].to_owned()),
            );
        }
    }
}

/// Pushes `text[range]` as a [`TextPart::Text`], unless it is empty.
///
/// `span` is the span of the whole line, which the range is an offset into.
fn push_text(
    parts: &mut Vec<Spanned<TextPart>>,
    text: &str,
    span: Span,
    range: std::ops::Range<usize>,
) {
    if range.is_empty() {
        return;
    }

    let part_span = Span::from_length(span.start + range.start, range.len());
    parts.push(Spanned::new(
        TextPart::Text(unescape(&text[range])),
        part_span,
    ));
}

pub(super) fn is_valid_ident(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Pushes the calls of the inline expressions of `text` onto `calls`.
fn text_calls<'a>(text: &'a [Spanned<TextPart>], calls: &mut Vec<Call<'a>>) {
    for part in text {
        if let TextPart::Expression(expr) = &part.value {
            expr_calls(expr, part.span, calls);
        }
    }
}

/// Pushes every call `expr` holds onto `calls`, itself included when it is
/// one. `span` is the span of `expr`.
fn expr_calls<'a>(expr: &'a Expr, span: Span, calls: &mut Vec<Call<'a>>) {
    let mut inner = |expr: &'a Spanned<Expr>| expr_calls(&expr.value, expr.span, calls);

    match expr {
        Expr::Function { name, args } => {
            calls.push(Call {
                name,
                args: args.len(),
                span,
            });
            for arg in args {
                expr_calls(&arg.value, arg.span, calls);
            }
        }
        Expr::Attr { base, .. } => inner(base),
        Expr::LitteralDict(entries) => {
            for (_, value) in entries {
                expr_calls(&value.value, value.span, calls);
            }
        }
        Expr::Unary { rhs, .. } => inner(rhs),
        Expr::Binary { lhs, rhs, .. } => {
            expr_calls(&lhs.value, lhs.span, calls);
            expr_calls(&rhs.value, rhs.span, calls);
        }
        Expr::Var { .. } | Expr::Litteral { .. } | Expr::Error => {}
    }
}

/// Pushes every statement of `stmts` onto `all`, each one right before the
/// statements of the blocks it opens.
fn flatten_stmts<'a>(stmts: &'a [Stmt], all: &mut Vec<&'a Stmt>) {
    for stmt in stmts {
        all.push(stmt);

        match &stmt.kind {
            StmtKind::Choice { choices } => {
                for choice in choices {
                    flatten_stmts(&choice.body, all);
                }
            }
            StmtKind::If {
                branches,
                otherwise,
            } => {
                for branch in branches {
                    flatten_stmts(&branch.body, all);
                }
                if let Some(body) = otherwise {
                    flatten_stmts(body, all);
                }
            }
            StmtKind::While { body, .. } => flatten_stmts(body, all),
            StmtKind::Say { .. }
            | StmtKind::Jump(_)
            | StmtKind::Command { .. }
            | StmtKind::Set { .. }
            | StmtKind::Break
            | StmtKind::Continue => {}
        }
    }
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
            StmtKind::Command { name, args, .. } => {
                (name.value, args.into_iter().map(|a| a.value).collect())
            }
            other => panic!("expected a command, got {other:?}"),
        }
    }

    /// Whether the dialogue waits on the command of `line`.
    fn awaited(line: &str) -> bool {
        match only_stmt(&in_node(line)) {
            StmtKind::Command { awaited, .. } => awaited,
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

    /// Parts of the only line of `src`, a literal as `"text"` and an inline
    /// expression as `[]`.
    fn parts(line: &str) -> Vec<String> {
        let StmtKind::Say { text, .. } = only_stmt(&in_node(line)) else {
            panic!("expected a line of dialogue");
        };

        text.into_iter()
            .map(|p| match p.value {
                TextPart::Text(text) => format!("{text:?}"),
                TextPart::Expression(_) => "[]".to_owned(),
            })
            .collect()
    }

    #[test]
    fn a_line_without_a_bracket_is_a_single_part() {
        assert_eq!(parts("Alice: Hello!"), [r#""Hello!""#]);
    }

    #[test]
    fn an_inline_expression_splits_the_line_it_sits_in() {
        assert_eq!(
            parts("Alice: Hello [$name]!"),
            [r#""Hello ""#, "[]", r#""!""#]
        );
        assert_eq!(parts("Alice: [$a] and [$b]"), ["[]", r#"" and ""#, "[]"]);
        assert_eq!(parts("Alice: [$name]"), ["[]"]);
        assert_eq!(parts("Alice: [$a][$b]"), ["[]", "[]"]);
    }

    #[test]
    fn an_inline_expression_reads_a_whole_expression() {
        assert_eq!(
            parts("Alice: I have [$money * 100] cents"),
            [r#""I have ""#, "[]", r#"" cents""#]
        );
        assert!(codes(&in_node("Alice: I have [$money * 100] cents")).is_empty());
    }

    #[test]
    fn error_on_text_left_after_an_inline_expression() {
        assert_eq!(
            codes(&in_node("Alice: Hello [$name oups]!")),
            ["unexpected-text-in-bracket"]
        );
    }

    #[test]
    fn error_on_an_inline_expression_with_nothing_in_it() {
        assert_eq!(codes(&in_node("Alice: Hello []!")), ["expected-expression"]);
    }

    /// The `[` is kept as text, so the line still says something.
    #[test]
    fn error_on_an_inline_expression_the_line_never_closes() {
        assert_eq!(codes(&in_node("Alice: Hello [$name")), ["unclosed-bracket"]);
        assert_eq!(parts("Alice: Hello [$name"), [r#""Hello ""#, r#""[$name""#]);
    }

    #[test]
    fn a_choice_reads_its_inline_expressions_too() {
        let StmtKind::Choice { choices } = only_stmt(&in_node("-> Pay [$price] gold")) else {
            panic!("expected a choice group");
        };

        assert_eq!(choices.len(), 1);
        assert_eq!(choices[0].text.len(), 3);
        assert!(matches!(choices[0].text[1].value, TextPart::Expression(_)));
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
        assert!(matches!(
            &choices[0].text[..],
            [Spanned {
                value: TextPart::Text(text),
                ..
            }] if text == "A"
        ));
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

    /// `await` is a word of the language, so the command it marks is the one
    /// that follows, and the flag rides on the statement.
    #[test]
    fn a_command_can_be_awaited() {
        assert!(!awaited(">> play(1)"));
        assert!(awaited(">> await play(1)"));
        assert_eq!(command(">> await play(1)").0, "play");
    }

    /// The space after `await` is a separator, not part of the name.
    #[test]
    fn an_awaited_command_ignores_the_space_after_the_keyword() {
        assert_eq!(command(">> await   play(1)").0, "play");
        assert!(awaited(">> await   play(1)"));
    }

    /// The keyword is dropped from the spans too, so a diagnostic still points
    /// at the offending text rather than a few bytes to its left.
    #[test]
    fn an_awaited_command_keeps_its_spans_on_the_source() {
        let src = &in_filled_node(">> await play(1, 1..2)");
        let parsed = parse(src);
        let diag = parsed.diagnostics.iter().next().unwrap();

        assert_eq!(&src[diag.span.start..diag.span.end], "1..2");
    }

    /// A word that merely starts with `await` is a command like any other.
    #[test]
    fn a_command_whose_name_starts_with_await_is_not_awaited() {
        assert_eq!(command(">> awaited_thing(1)").0, "awaited_thing");
        assert!(!awaited(">> awaited_thing(1)"));
    }

    #[test]
    fn error_on_a_command_named_after_a_reserved_word() {
        assert_eq!(
            codes(&in_filled_node(">> await()")),
            ["reserved-command-name"]
        );
        // `await` alone marks nothing: the name it expects is missing.
        assert_eq!(
            codes(&in_filled_node(">> await")),
            ["reserved-command-name"]
        );
        assert_eq!(
            codes(&in_filled_node(">> await await()")),
            ["reserved-command-name"]
        );
    }

    /// `await` with nothing after it is a command without a name, as `>>` is.
    #[test]
    fn error_on_an_awaited_command_without_a_name() {
        assert_eq!(codes(&in_filled_node(">> await ()")), ["empty-command"]);
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

    /// The arguments of a command go through the same type check as any other
    /// expression, at the place they are written.
    #[test]
    fn error_on_an_argument_of_the_wrong_type() {
        assert_eq!(
            codes(&in_filled_node(">> play(not 1)")),
            ["invalid-unary-operand"]
        );
        assert_eq!(
            codes(&in_filled_node(r#">> play("a" - 1)"#)),
            ["invalid-binary-operands"]
        );
    }

    /// Name and value of an assignment, the value as the text it covers.
    fn set(line: &str) -> (String, Vec<String>, String) {
        let src = in_node(line);
        match only_stmt(&src) {
            StmtKind::Set { name, attrs, value } => {
                let attrs = attrs.into_iter().map(|s| s.value).collect();
                (
                    name.value,
                    attrs,
                    src[value.span.start..value.span.end].to_owned(),
                )
            }
            other => panic!("expected an assignment, got {other:?}"),
        }
    }

    #[test]
    fn an_assignment_takes_a_name_and_an_expression() {
        assert_eq!(
            set("[let $gold = 10]"),
            ("gold".to_string(), vec![], "10".to_string())
        );
        assert_eq!(
            set("[let $gold = $gold + 10]"),
            ("gold".to_string(), vec![], "$gold + 10".to_string())
        );
    }

    #[test]
    fn an_assignment_takes_a_name_and_attrs() {
        assert_eq!(
            set("[let $money.gold = 10]"),
            (
                "money".to_string(),
                vec!["gold".to_string()],
                "10".to_string()
            )
        );
        assert_eq!(
            set("[let $boss.hp.remaining = 10]"),
            (
                "boss".to_string(),
                vec!["hp".to_string(), "remaining".to_string()],
                "10".to_string()
            )
        );
    }

    #[test]
    fn spaces_around_an_assignment_are_ignored() {
        assert_eq!(
            set("[let   $gold=10  ]"),
            ("gold".to_string(), vec![], "10".to_string())
        );
    }

    #[test]
    fn error_on_an_assignment_that_is_never_closed() {
        assert_eq!(
            codes(&in_filled_node("[let $gold = 10")),
            ["unclosed-bracket"]
        );
    }

    #[test]
    fn error_on_something_that_is_not_an_assignment() {
        for line in [
            "[let $gold + 1]",
            "[let gold = 1]",
            "[let]",
            "[let $gold == 1]",
        ] {
            assert_eq!(
                codes(&in_filled_node(line)),
                ["expected-assignment"],
                "{line}"
            );
        }
    }

    #[test]
    fn error_on_an_assignment_to_a_nameless_variable() {
        assert_eq!(
            codes(&in_filled_node("[let $ = 1]")),
            ["empty-variable-name", "expected-assignment"]
        );
    }

    #[test]
    fn error_on_text_left_after_the_value() {
        assert_eq!(
            codes(&in_filled_node("[let $gold = 10 20]")),
            ["unexpected-text-in-bracket"]
        );
    }

    #[test]
    fn the_value_of_an_assignment_is_checked() {
        assert_eq!(
            codes(&in_filled_node("[let $gold = 1 +]")),
            ["expected-expression"]
        );
        assert_eq!(
            codes(&in_filled_node(r#"[let $gold = "a" - 1]"#)),
            ["invalid-binary-operands"]
        );
    }

    #[test]
    fn a_broken_assignment_costs_exactly_one_line() {
        let parsed = parse(":= start\n[let $gold = 10\nAlice: ok\n---\n");

        assert_eq!(parsed.nodes[0].body.len(), 1);
        assert!(matches!(parsed.nodes[0].body[0].kind, StmtKind::Say { .. }));
    }

    /// Conditions of an `[if]` group, and whether it has an `[else]`.
    fn branches(src: &str) -> (Vec<String>, bool) {
        let full = in_node(src);
        match only_stmt(&full) {
            StmtKind::If {
                branches,
                otherwise,
            } => (
                branches
                    .iter()
                    .map(|b| full[b.condition.span.start..b.condition.span.end].to_owned())
                    .collect(),
                otherwise.is_some(),
            ),
            other => panic!("expected an if, got {other:?}"),
        }
    }

    #[test]
    fn a_condition_owns_the_block_indented_under_it() {
        let stmt = only_stmt(&in_node("[if $gold > 5]\n    Alice: riche"));
        let StmtKind::If { branches, .. } = stmt else {
            panic!("expected an if");
        };

        assert_eq!(branches.len(), 1);
        assert_eq!(branches[0].body.len(), 1);
        assert!(matches!(branches[0].body[0].kind, StmtKind::Say { .. }));
    }

    #[test]
    fn the_branches_of_a_group_are_kept_in_order() {
        assert_eq!(
            branches(
                "[if $gold > 5]\n    Alice: a\n[elif $gold > 1]\n    Alice: b\n[else]\n    Alice: c"
            ),
            (vec!["$gold > 5".to_string(), "$gold > 1".to_string()], true)
        );
    }

    #[test]
    fn a_condition_can_stand_without_an_else() {
        assert_eq!(
            branches("[if true]\n    Alice: a"),
            (vec!["true".to_string()], false)
        );
    }

    /// A second `[if]` at the same indentation opens its own group instead of
    /// joining the one before it.
    #[test]
    fn two_conditions_in_a_row_are_two_groups() {
        let parsed = parse(":= start\n[if true]\n    Alice: a\n[if false]\n    Alice: b\n---\n");

        assert_eq!(parsed.nodes[0].body.len(), 2);
        assert!(parsed.diagnostics.iter().next().is_none());
    }

    #[test]
    fn a_group_can_hold_another_one() {
        let stmt = only_stmt(&in_node(
            "[if true]\n    [if false]\n        Alice: a\n    [else]\n        Alice: b",
        ));
        let StmtKind::If { branches, .. } = stmt else {
            panic!("expected an if");
        };

        assert!(matches!(branches[0].body[0].kind, StmtKind::If { .. }));
    }

    #[test]
    fn error_on_a_condition_that_is_never_closed() {
        assert_eq!(
            codes(&in_filled_node("[if $gold > 5")),
            ["unclosed-bracket"]
        );
    }

    /// A condition guards a block, so it has to be a `bool`. A variable or a
    /// call has no type yet, and is left to the VM.
    #[test]
    fn error_on_a_condition_that_is_not_a_bool() {
        assert_eq!(
            codes(&in_filled_node("[if 1 + 1]\n    Alice: a")),
            ["condition-not-a-bool"]
        );
        assert_eq!(
            codes(&in_filled_node("[if \"oui\"]\n    Alice: a")),
            ["condition-not-a-bool"]
        );
        // A variable or a call has no type before the dialogue runs.
        assert!(codes(&in_filled_node("[if $gold + 1 > 2]\n    Alice: a")).is_empty());
        assert!(codes(&in_filled_node("[if $flag]\n    Alice: a")).is_empty());
        assert!(codes(&in_filled_node("[if is_open()]\n    Alice: a")).is_empty());
    }

    #[test]
    fn error_on_text_left_in_a_branch() {
        assert_eq!(
            codes(&in_filled_node("[if true 1]\n    Alice: a")),
            ["unexpected-text-in-bracket"]
        );
        assert_eq!(
            codes(&in_filled_node(
                "[if true]\n    Alice: a\n[else oups]\n    Alice: b"
            )),
            ["unexpected-text-in-bracket"]
        );
    }

    #[test]
    fn error_on_a_branch_with_no_condition_before_it() {
        assert_eq!(
            codes(&in_filled_node("[elif true]\n    Alice: a")),
            ["stray-branch"]
        );
        assert_eq!(
            codes(&in_filled_node("[else]\n    Alice: a")),
            ["stray-branch"]
        );
    }

    #[test]
    fn error_on_a_branch_after_the_else() {
        assert_eq!(
            codes(&in_filled_node(
                "[if true]\n    Alice: a\n[else]\n    Alice: b\n[elif false]\n    Alice: c"
            )),
            ["branch-after-else"]
        );
        assert_eq!(
            codes(&in_filled_node(
                "[if true]\n    Alice: a\n[else]\n    Alice: b\n[else]\n    Alice: c"
            )),
            ["branch-after-else"]
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

    /// Speaker, text and tags of the only line of `src`, the tags as they are written.
    fn said(line: &str) -> (Option<String>, String, Vec<String>) {
        let StmtKind::Say {
            speaker,
            text,
            tags,
        } = only_stmt(&in_node(line))
        else {
            panic!("expected a line");
        };

        let text = text
            .iter()
            .map(|part| match &part.value {
                TextPart::Text(text) => text.as_str(),
                TextPart::Expression(_) => "[]",
            })
            .collect();
        let tags = tags.iter().map(|tag| tag.value.to_string()).collect();

        (speaker.map(|s| s.value), text, tags)
    }

    #[test]
    fn tags_are_read_at_both_ends_of_a_line() {
        assert_eq!(
            said("Alice: #angry I am angry #sound:alice_01"),
            (
                Some("Alice".into()),
                "I am angry".into(),
                vec!["#angry".into(), "#sound:alice_01".into()]
            )
        );
        assert!(codes(&in_node("Alice: #angry I am angry #sound:alice_01")).is_empty());
    }

    #[test]
    fn a_line_without_a_speaker_takes_tags_too() {
        assert_eq!(
            said("#angry The door slams. #sound:door"),
            (
                None,
                "The door slams.".into(),
                vec!["#angry".into(), "#sound:door".into()]
            )
        );
    }

    #[test]
    fn a_tag_with_a_value_is_not_read_as_a_speaker() {
        assert_eq!(
            said("#mood:angry I am angry"),
            (None, "I am angry".into(), vec!["#mood:angry".into()])
        );
        assert_eq!(
            said("Alice: #mood:angry I am angry"),
            (
                Some("Alice".into()),
                "I am angry".into(),
                vec!["#mood:angry".into()]
            )
        );
    }

    #[test]
    fn a_speaker_can_hold_a_hash_that_opens_no_tag() {
        assert_eq!(
            said("Agent #2: Bonjour"),
            (Some("Agent #2".into()), "Bonjour".into(), vec![])
        );
    }

    #[test]
    fn a_line_can_be_nothing_but_tags() {
        assert_eq!(
            said("Alice: #angry"),
            (Some("Alice".into()), String::new(), vec!["#angry".into()])
        );
    }

    #[test]
    fn tags_go_around_the_inline_expressions() {
        assert_eq!(
            said("Alice: #a Bonjour [$nom] ! #b"),
            (
                Some("Alice".into()),
                "Bonjour [] !".into(),
                vec!["#a".into(), "#b".into()]
            )
        );
        assert_eq!(
            said("Alice: #a [$nom] #b"),
            (
                Some("Alice".into()),
                "[]".into(),
                vec!["#a".into(), "#b".into()]
            )
        );
    }

    #[test]
    fn what_is_inside_the_brackets_is_never_a_tag() {
        let line = r##"Alice: [upper("#x")] et [$a has "#y"]"##;

        assert_eq!(said(line).2, Vec::<String>::new());
        assert!(codes(&in_node(line)).is_empty());
    }

    #[test]
    fn warning_on_a_tag_in_the_middle_of_a_line() {
        let line = "Alice: I am #angry today";

        assert_eq!(
            said(line),
            (Some("Alice".into()), "I am #angry today".into(), vec![])
        );
        assert_eq!(codes(&in_node(line)), ["misplaced-tag"]);
    }

    #[test]
    fn warning_on_a_word_that_opens_like_a_tag_and_is_not_one() {
        assert_eq!(codes(&in_node("Alice: Salut #mood:")), ["misplaced-tag"]);
        assert_eq!(codes(&in_node("Alice: #mood:a:b Salut")), ["misplaced-tag"]);
    }

    #[test]
    fn a_misplaced_tag_does_not_make_a_speaker_of_what_comes_before() {
        let line = "I am #mood:angry today";

        assert_eq!(said(line), (None, "I am #mood:angry today".into(), vec![]));
        assert_eq!(codes(&in_node(line)), ["misplaced-tag"]);
    }

    #[test]
    fn a_misplaced_tag_is_only_a_warning() {
        let parsed = parse(&in_node("Alice: I am #angry today"));

        assert_eq!(parsed.diagnostics.errors(), 0);
    }

    #[test]
    fn the_warning_points_at_the_word() {
        let src = in_node("Alice: I am #angry, today");
        let parsed = parse(&src);
        let diag = parsed.diagnostics.iter().next().expect("one diagnostic");

        assert_eq!(&src[diag.span.start..diag.span.end], "#angry,");
    }

    #[test]
    fn an_escaped_hash_is_text() {
        let line = r"Alice: \#angry is trending, says \#bob";

        assert_eq!(
            said(line),
            (
                Some("Alice".into()),
                "#angry is trending, says #bob".into(),
                vec![]
            )
        );
        assert!(codes(&in_node(line)).is_empty());
    }

    #[test]
    fn a_hash_that_opens_no_tag_is_text() {
        let line = "Alice: We are #1 in C# and F#";

        assert_eq!(
            said(line),
            (
                Some("Alice".into()),
                "We are #1 in C# and F#".into(),
                vec![]
            )
        );
        assert!(codes(&in_node(line)).is_empty());
    }

    #[test]
    fn warning_on_a_tag_given_twice() {
        let line = "Alice: #mood:angry Hi #mood:sad";

        assert_eq!(said(line).2, ["#mood:angry", "#mood:sad"].map(String::from));
        assert_eq!(codes(&in_node(line)), ["duplicate-tag"]);
        assert_eq!(parse(&in_node(line)).diagnostics.errors(), 0);
    }

    #[test]
    fn two_different_tags_are_not_a_duplicate() {
        assert!(codes(&in_node("Alice: #mood:angry Hi #sound:angry")).is_empty());
    }

    #[test]
    fn the_spans_of_the_tags_point_at_them() {
        let src = in_node("Alice: #angry Hi #sound:a1");
        let StmtKind::Say { tags, .. } = only_stmt(&src) else {
            panic!("expected a line");
        };

        let words: Vec<_> = tags
            .iter()
            .map(|tag| &src[tag.span.start..tag.span.end])
            .collect();
        assert_eq!(words, ["#angry", "#sound:a1"]);
    }

    fn choices_of(src: &str) -> Vec<(String, Vec<String>)> {
        let parsed = parse(src);
        let StmtKind::Choice { choices } = &parsed.nodes[0].body[0].kind else {
            panic!("expected a choice");
        };

        choices
            .iter()
            .map(|choice| {
                let text = choice
                    .text
                    .iter()
                    .map(|part| match &part.value {
                        TextPart::Text(text) => text.as_str(),
                        TextPart::Expression(_) => "[]",
                    })
                    .collect();
                let tags = choice.tags.iter().map(|t| t.value.to_string()).collect();
                (text, tags)
            })
            .collect()
    }

    #[test]
    fn a_choice_takes_tags_like_a_line_does() {
        let src = ":= start
-> #hostile Attaquer [$nom] #cost:3
    Alice: ok
-> Rester
    Alice: ok
---
";

        assert_eq!(
            choices_of(src),
            [
                (
                    "Attaquer []".to_owned(),
                    vec!["#hostile".to_owned(), "#cost:3".to_owned()]
                ),
                ("Rester".to_owned(), vec![]),
            ]
        );
        assert!(codes(src).is_empty());
    }

    #[test]
    fn warnings_on_the_tags_of_a_choice() {
        let src = ":= start
-> Je #doute un peu
    Alice: ok
-> #ton:sec Partir #ton:doux
    Alice: ok
---
";

        assert_eq!(codes(src), ["misplaced-tag", "duplicate-tag"]);
        assert_eq!(parse(src).diagnostics.errors(), 0);
    }

    #[test]
    fn error_on_a_choice_made_of_tags_alone() {
        let src = ":= start\n-> #seul\n    Alice: ok\n-> Rester\n    Alice: ok\n---\n";

        assert_eq!(codes(src), ["empty-choice-text"]);
    }

    #[test]
    fn a_hash_that_opens_no_tag_is_fine_on_a_choice() {
        let src = ":= start\n-> Prendre le #1\n    Alice: ok\n-> Rester\n    Alice: ok\n---\n";

        assert!(codes(src).is_empty());
    }

    /// What each statement of the only node of `src` is, flattened.
    fn flattened(src: &str) -> Vec<String> {
        parse(src).nodes[0]
            .all_statements()
            .into_iter()
            .map(|stmt| match &stmt.kind {
                StmtKind::Say { speaker, .. } => {
                    format!("say {}", speaker.as_ref().map_or("-", |s| &s.value))
                }
                StmtKind::Choice { .. } => "choice".to_owned(),
                StmtKind::Jump(name) => format!("jump {}", name.value),
                StmtKind::Command { name, .. } => format!("command {}", name.value),
                StmtKind::Set { name, .. } => format!("set {}", name.value),
                StmtKind::If { .. } => "if".to_owned(),
                StmtKind::While { .. } => "while".to_owned(),
                StmtKind::Break => "break".to_owned(),
                StmtKind::Continue => "continue".to_owned(),
            })
            .collect()
    }

    #[test]
    fn all_statements_of_a_flat_node_are_its_body() {
        assert_eq!(
            flattened(":= start\nAlice: un\n>> wave()\n[let $n = 1]\n=> END\n---\n"),
            ["say Alice", "command wave", "set n", "jump END"]
        );
    }

    #[test]
    fn all_statements_open_every_kind_of_block() {
        let src = ":= start
[let $n = 0]
[if $n > 0]
    A: if
[elif $n < 0]
    B: elif
[else]
    C: else
[while $n < 2]
    D: while
    [break]
-> un
    E: choice
-> deux
    F: choice
G: end
---
";

        assert_eq!(
            flattened(src),
            [
                "set n", "if", "say A", "say B", "say C", "while", "say D", "break", "choice",
                "say E", "say F", "say G",
            ]
        );
    }

    #[test]
    fn all_statements_go_as_deep_as_the_blocks_do() {
        let src = ":= start
[let $n = 0]
[while $n < 2]
    [if $n == 0]
        -> un
            [while true]
                A: deep
                [continue]
        -> deux
            B: deep
---
";

        assert_eq!(
            flattened(src),
            [
                "set n", "while", "if", "choice", "while", "say A", "continue", "say B",
            ]
        );
    }

    /// Name and number of arguments of every call of the only node of `src`.
    fn calls(src: &str) -> Vec<(String, usize)> {
        parse(src).nodes[0]
            .all_calls()
            .into_iter()
            .map(|call| (call.name.value.clone(), call.args))
            .collect()
    }

    #[test]
    fn all_calls_look_wherever_an_expression_can_be() {
        let src = ":= start
[let $n = in_let()]
Alice: Hi [in_line(1)] and [in_line_again(1, 2)]
>> command(in_command(1, 2, 3))
[if in_if()]
    Alice: ok
[elif in_elif()]
    Alice: ok
[while in_while()]
    Alice: ok
-> Take [in_choice()]
    Alice: [in_choice_body()]
-> Leave
    Alice: ok
---
";

        assert_eq!(
            calls(src),
            [
                ("in_let".to_owned(), 0),
                ("in_line".to_owned(), 1),
                ("in_line_again".to_owned(), 2),
                ("in_command".to_owned(), 3),
                ("in_if".to_owned(), 0),
                ("in_elif".to_owned(), 0),
                ("in_while".to_owned(), 0),
                ("in_choice".to_owned(), 0),
                ("in_choice_body".to_owned(), 0),
            ]
        );
    }

    #[test]
    fn all_calls_go_through_every_kind_of_expression() {
        let src = ":= start
[let $n = outer(inner(1), 2) + not_called]
[let $d = {a: in_dict(), b: {c: nested_dict(1)}}]
[let $x = in_attr().hp]
[let $y = not in_unary() and (in_paren(1) == -in_neg())]
[let $z = $n has in_has()]
---
";

        assert_eq!(
            calls(src),
            [
                ("outer".to_owned(), 2),
                ("inner".to_owned(), 1),
                ("in_dict".to_owned(), 0),
                ("nested_dict".to_owned(), 1),
                ("in_attr".to_owned(), 0),
                ("in_unary".to_owned(), 0),
                ("in_paren".to_owned(), 1),
                ("in_neg".to_owned(), 0),
                ("in_has".to_owned(), 0),
            ]
        );
    }

    /// A `>>` command is not a function, even when it shares its name.
    #[test]
    fn a_command_is_not_a_call() {
        assert!(calls(":= start\n>> upper()\n>> await wave(1, 2)\n---\n").is_empty());
    }

    #[test]
    fn the_span_of_a_call_covers_it_whole() {
        let src = ":= start\n[let $n = 1 + outer(inner(1), 2)]\nAlice: [ spaced( 1 ) ]\n---\n";
        let parsed = parse(src);

        let spans: Vec<_> = parsed.nodes[0]
            .all_calls()
            .into_iter()
            .map(|call| &src[call.span.start..call.span.end])
            .collect();
        assert_eq!(spans, ["outer(inner(1), 2)", "inner(1)", " spaced( 1 ) "]);
    }

    /// Codes of `src`, and the text each diagnostic points at.
    fn reported(src: &str) -> Vec<(&'static str, String)> {
        parse(src)
            .diagnostics
            .iter()
            .map(|d| (d.kind.code(), src[d.span.start..d.span.end].to_owned()))
            .collect()
    }

    #[test]
    fn a_builtin_given_the_arguments_it_takes_is_fine() {
        let src = ":= start
[let $n = max(1, 2, 3)]
Alice: [upper(\"a\")] has [len(\"abc\")] and [min(1, abs(-2))]
---
";

        assert!(codes(src).is_empty(), "{:?}", codes(src));
    }

    #[test]
    fn error_on_a_builtin_given_the_wrong_number_of_arguments() {
        let src = ":= start
[let $n = max(1)]
Alice: [upper()] and [len(\"a\", \"b\")]
---
";

        assert_eq!(
            reported(src),
            [
                ("builtin-arity", "max(1)".to_owned()),
                ("builtin-arity", "upper()".to_owned()),
                ("builtin-arity", "len(\"a\", \"b\")".to_owned()),
            ]
        );
        assert_eq!(parse(src).diagnostics.errors(), 3);
    }

    #[test]
    fn a_builtin_is_checked_wherever_it_is_called() {
        let src = ":= start
[if upper() == \"A\"]
    >> play(lower())
[while len() > 0]
    -> Take [abs()]
        Alice: [upper(capitalize())]
    -> Leave
        Alice: ok
---
";

        let names: Vec<_> = parse(src)
            .diagnostics
            .iter()
            .filter_map(|d| match &d.kind {
                DiagnosticKind::BuiltinArity { name, .. } => Some(name.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(names, ["upper", "lower", "len", "abs", "capitalize"]);
    }

    /// What the game adds is not known here: its arity is the business of
    /// the schema.
    #[test]
    fn a_function_that_is_not_a_builtin_is_not_checked() {
        let src = ":= start\nAlice: [gold()] and [gold(1, 2, 3)]\n---\n";

        assert!(codes(src).is_empty());
    }

    #[test]
    fn a_command_named_like_a_builtin_is_not_checked() {
        assert!(codes(":= start\n>> upper()\n---\n").is_empty());
    }

    #[test]
    fn the_builtin_error_says_what_was_expected_and_what_was_given() {
        let parsed = parse(":= start\nAlice: [upper()] and [max(1)]\n---\n");

        let messages: Vec<_> = parsed
            .diagnostics
            .iter()
            .map(|d| d.kind.to_string())
            .collect();
        assert_eq!(
            messages,
            [
                "builtin `upper` expects 1 argument, got 0",
                "builtin `max` expects at least 2 arguments, got 1",
            ]
        );
    }

    /// An error stops the compilation: the dialogue would fail on that call.
    #[test]
    fn a_builtin_called_wrong_does_not_compile() {
        let file = crate::RepliqueFile::from_source(":= start\nAlice: [upper()]\n---\n");

        assert!(file.has_errors());
        assert!(file.dialogue.is_none());
    }
}
