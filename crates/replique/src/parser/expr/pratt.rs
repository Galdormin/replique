use crate::parser::{
    Span, Spanned,
    ast::Value,
    diagnostic::{DiagnosticKind, Diagnostics, Label},
    expr::{
        BinaryOp, Expr, UnaryOp,
        lexer::{Lexed, Token, lex},
    },
};

/// Binding power of the unary operators, above every infix one.
const PREFIX_PRIO: u8 = 11;

/// Reads the expression `src` holds, and checks the types of its operators.
pub(crate) fn parse(src: &Spanned<&str>, diags: &mut Diagnostics) -> Spanned<Expr> {
    parse_with_trailing(src, diags).0
}

/// Same, plus the span of what follows the expression when it does not read
/// the whole of `src`. What to make of that text is up to the caller.
pub(crate) fn parse_with_trailing(
    src: &Spanned<&str>,
    diags: &mut Diagnostics,
) -> (Spanned<Expr>, Option<Span>) {
    let mut parser = ExprParser::new(src, diags);

    let expr = parser.expr(0);
    expr.value.is_type_valid(parser.diags);
    let trailing = parser.trailing(src);

    (expr, trailing)
}

/// What `$name = <expression>` holds, once read.
pub(crate) struct Assignment {
    /// Name of the variable, without its `$`. Its span covers the sigil.
    pub name: Spanned<String>,
    /// Name of the path of attibutes
    pub attrs: Vec<Spanned<String>>,
    pub value: Spanned<Expr>,
    /// What the expression did not read, from the first token left to the end
    /// of the source. Reported by the caller, which knows what statement it
    /// is reading.
    pub trailing: Option<Span>,
}

/// Reads `$name = <expression>` out of `src`.
pub(crate) fn parse_assignment(src: &Spanned<&str>, diags: &mut Diagnostics) -> Option<Assignment> {
    let mut parser = ExprParser::new(src, diags);

    let name = match parser.peek()? {
        Spanned {
            value: Token::Var(name),
            span,
        } => Spanned::new(name.clone(), *span),
        _ => return None,
    };

    let mut attrs = vec![];
    loop {
        parser.bump();
        match parser.peek()? {
            Spanned {
                value: Token::Assign,
                ..
            } => break,
            Spanned {
                value: Token::Attr(attr),
                span,
            } => attrs.push(Spanned::new(attr.clone(), *span)),
            _ => return None,
        }
    }
    parser.bump();

    let value = parser.expr(0);
    value.value.is_type_valid(parser.diags);
    let trailing = parser.trailing(src);

    Some(Assignment {
        name,
        attrs,
        value,
        trailing,
    })
}

pub struct ExprParser<'a> {
    lexed: Lexed,
    pos: usize,
    diags: &'a mut Diagnostics,
    /// Empty span at the end of the source.
    end_span: Span,
}

impl<'a> ExprParser<'a> {
    pub(crate) fn new(src: &Spanned<&str>, diags: &'a mut Diagnostics) -> Self {
        Self {
            lexed: lex(src, diags),
            pos: 0,
            diags,
            end_span: Span::from_length(src.span.end, 0),
        }
    }

    fn peek(&self) -> Option<&Spanned<Token>> {
        self.lexed.tokens.get(self.pos)
    }

    /// From the token left to read, if any, to the end of `src`.
    fn trailing(&self, src: &Spanned<&str>) -> Option<Span> {
        self.peek_span()
            .map(|span| Span::new(span.start, src.span.end))
    }

    /// Span of the next token, without consuming it.
    fn peek_span(&self) -> Option<Span> {
        self.peek().map(|tok| tok.span)
    }

    /// Token of the next token, without consuming it.
    fn peek_token(&self) -> Option<&Token> {
        self.peek().map(|tok| &tok.value)
    }

    fn bump(&mut self) {
        self.pos += 1;
    }

    /// Consumes the next token when it is the one wanted, and gives its span.
    fn eat_if(&mut self, wanted: impl FnOnce(&Token) -> bool) -> Option<Span> {
        let span = self.peek().filter(|tok| wanted(&tok.value))?.span;
        self.bump();
        Some(span)
    }

    pub(crate) fn expr(&mut self, min_prio: u8) -> Spanned<Expr> {
        let mut lhs = self.prefix();

        while let Some((op, lprio, rprio)) = self.peek().and_then(infix) {
            if lprio < min_prio {
                break;
            }

            self.bump();
            let rhs = self.expr(rprio);
            let expr_span = lhs.span.join(rhs.span);
            lhs = Spanned::new(
                Expr::Binary {
                    op,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                },
                expr_span,
            );
        }

        lhs
    }

    /// Read the first expression of the line
    fn prefix(&mut self) -> Spanned<Expr> {
        let base = self.primary();
        self.attrs(base)
    }

    /// Return the base of a potential attr
    fn primary(&mut self) -> Spanned<Expr> {
        let Some(Spanned { value, span }) = self.peek().cloned() else {
            self.diags
                .push(self.end_span, DiagnosticKind::ExpectedExpression);
            return error(self.end_span);
        };

        self.bump();

        match value {
            Token::Var(name) => Spanned::new(Expr::Var { name }, span),
            Token::Func(name) => self.parse_call(Spanned::new(name, span)),
            Token::Ident(val) => match val.as_str() {
                "true" => lit(Value::Bool(true), span),
                "false" => lit(Value::Bool(false), span),
                _ => lit(Value::String(val), span),
            },
            Token::Float(val) => lit(Value::Float(val), span),
            Token::Int(val) => lit(Value::Int(val), span),
            Token::Str(val) => lit(Value::String(val), span),

            // Unary op
            Token::Not => self.unary(Spanned::new(UnaryOp::Not, span)),
            Token::Minus => self.unary(Spanned::new(UnaryOp::Neg, span)),

            // Parenthesis
            Token::LParen => {
                let expr = self.expr(0);
                if matches!(self.peek_token(), Some(&Token::RParen)) {
                    self.bump();
                } else {
                    self.diags.push(span, DiagnosticKind::UnclosedParenthesis);
                }
                expr
            }

            // Litteral dict
            Token::LBrace => self.parse_litteral_dict(span),

            _ => {
                self.diags.push(span, DiagnosticKind::ExpectedExpression);
                error(span)
            }
        }
    }

    /// Attaches every `.attr` to the base
    fn attrs(&mut self, base: Spanned<Expr>) -> Spanned<Expr> {
        let mut attrs = vec![];

        // Read all attr
        while let Some(Spanned {
            value: Token::Attr(name),
            span,
        }) = self.peek().cloned()
        {
            self.bump();
            attrs.push(Spanned::new(name, span));
        }

        let Some(last) = attrs.last() else {
            // No attr found
            return base;
        };
        let full_span = base.span.join(last.span);

        if !matches!(base.value, Expr::Var { .. } | Expr::Function { .. }) {
            self.diags
                .push(full_span, DiagnosticKind::AttributeOnNonVariable);
            return error(full_span);
        }

        Spanned::new(
            Expr::Attr {
                base: Box::new(base),
                attrs,
            },
            full_span,
        )
    }

    fn unary(&mut self, op: Spanned<UnaryOp>) -> Spanned<Expr> {
        let rhs = self.expr(PREFIX_PRIO);
        let full_span = op.span.join(rhs.span);
        Spanned::new(
            Expr::Unary {
                op,
                rhs: Box::new(rhs),
            },
            full_span,
        )
    }

    pub fn parse_call(&mut self, name: Spanned<String>) -> Spanned<Expr> {
        let name_span = name.span;
        let mut args = Vec::new();

        // `func()`, the only call with no argument. Everywhere else a `)` is
        // preceded by an argument, so a `,` just before it has none.
        if let Some(span) = self.eat_if(|tok| matches!(tok, Token::RParen)) {
            return Spanned::new(Expr::Function { name, args }, name_span.join(span));
        }

        loop {
            // An argument.
            match self.peek() {
                Some(Spanned {
                    value: Token::RParen | Token::Separator,
                    span,
                }) => {
                    let span = *span;
                    self.diags.push(span, DiagnosticKind::ExpectedExpression);
                    return self.abandon_call(name_span);
                }
                None => {
                    self.diags.push(name_span, DiagnosticKind::UnclosedCall);
                    return error(name_span);
                }
                _ => args.push(self.expr(0)),
            }

            // The `)` that ends the call, or the `,` before the next argument.
            if let Some(span) = self.eat_if(|tok| matches!(tok, Token::RParen)) {
                return Spanned::new(Expr::Function { name, args }, name_span.join(span));
            }
            if self.eat_if(|tok| matches!(tok, Token::Separator)).is_none() {
                return match self.peek_span() {
                    Some(span) => {
                        self.diags.push(span, DiagnosticKind::ExpectedSeparator);
                        self.abandon_call(name_span)
                    }
                    None => {
                        self.diags.push(name_span, DiagnosticKind::UnclosedCall);
                        error(name_span)
                    }
                };
            }
        }
    }

    /// Gives up on a call whose fault is already reported.
    /// Consume what is left before the end of the call `)` then return [`Expr::Error`].
    fn abandon_call(&mut self, name_span: Span) -> Spanned<Expr> {
        let end = self.skip_to_call_end().unwrap_or(name_span);
        error(name_span.join(end))
    }

    fn skip_to_call_end(&mut self) -> Option<Span> {
        let mut depth = 0usize;

        while let Some(tok) = self.peek() {
            let span = tok.span;
            let closing = match tok.value {
                Token::LParen | Token::Func(_) => {
                    depth += 1;
                    false
                }
                Token::RParen => true,
                _ => false,
            };

            self.bump();

            if closing {
                match depth.checked_sub(1) {
                    Some(outer) => depth = outer,
                    None => return Some(span),
                }
            }
        }

        None
    }

    /// Parse a litteral dict `{name: "Léon", age: 1}`.
    /// Do not consume the last `}`
    fn parse_litteral_dict(&mut self, open_span: Span) -> Spanned<Expr> {
        let mut attrs = Vec::new();

        // Empty dict
        if let Some(span) = self.eat_if(|tok| matches!(tok, Token::RBrace)) {
            return Spanned::new(Expr::LitteralDict(attrs), open_span.join(span));
        }

        loop {
            // Key
            let key = match self.peek() {
                Some(Spanned {
                    value: Token::Ident(k),
                    span,
                }) => Spanned::new(k.clone(), *span),
                Some(Spanned { span, .. }) => {
                    let span = *span;
                    self.diags.push(span, DiagnosticKind::ExpectedKey);
                    return self.abandon_dict(open_span);
                }
                None => {
                    self.diags.push(open_span, DiagnosticKind::UnclosedDict);
                    return error(open_span);
                }
            };
            self.bump();

            // Colon
            if self.eat_if(|tok| matches!(tok, Token::Colon)).is_none() {
                return match self.peek_span() {
                    Some(span) => {
                        self.diags.push(span, DiagnosticKind::ExpectedColon);
                        return self.abandon_dict(open_span);
                    }
                    None => {
                        self.diags.push(open_span, DiagnosticKind::UnclosedDict);
                        error(open_span)
                    }
                };
            }

            // Expression
            let val = match self.peek() {
                Some(Spanned {
                    value: Token::RBrace | Token::Separator,
                    span,
                }) => {
                    let span = *span;
                    self.diags.push(span, DiagnosticKind::ExpectedExpression);
                    return self.abandon_dict(open_span);
                }
                None => {
                    self.diags.push(open_span, DiagnosticKind::UnclosedDict);
                    return error(open_span);
                }
                _ => self.expr(0),
            };

            // Only the last value survives the conversion to a map, so a key
            // written twice is almost always a mistake rather than an override.
            if let Some((first, _)) = attrs.iter().find(|(k, _)| k.value == key.value) {
                self.diags.push_labeled(
                    key.span,
                    DiagnosticKind::DuplicateKey(key.value.clone()),
                    vec![Label {
                        span: first.span,
                        message: "first given here".to_owned(),
                    }],
                );
            }

            attrs.push((key, val));

            // Possible `}`
            if let Some(span) = self.eat_if(|tok| matches!(tok, Token::RBrace)) {
                return Spanned::new(Expr::LitteralDict(attrs), open_span.join(span));
            }

            // Separator
            if self.eat_if(|tok| matches!(tok, Token::Separator)).is_none() {
                return match self.peek_span() {
                    Some(span) => {
                        self.diags.push(span, DiagnosticKind::ExpectedSeparator);
                        self.abandon_dict(open_span)
                    }
                    None => {
                        self.diags.push(open_span, DiagnosticKind::UnclosedDict);
                        error(open_span)
                    }
                };
            }
        }
    }

    /// Gives up on a dict whose fault is already reported.
    /// Consume what is left before the end of the dict `}` then return [`Expr::Error`].
    fn abandon_dict(&mut self, open_span: Span) -> Spanned<Expr> {
        let end = self.skip_to_dict_end().unwrap_or(open_span);
        error(open_span.join(end))
    }

    fn skip_to_dict_end(&mut self) -> Option<Span> {
        let mut depth = 0usize;

        while let Some(tok) = self.peek() {
            let span = tok.span;
            let closing = match tok.value {
                Token::LBrace => {
                    depth += 1;
                    false
                }
                Token::RBrace => true,
                _ => false,
            };

            self.bump();

            if closing {
                match depth.checked_sub(1) {
                    Some(outer) => depth = outer,
                    None => return Some(span),
                }
            }
        }

        None
    }
}

/// Operator, left and right priority. The right one is the greater of
/// the two, which makes every operator left-associative.
fn infix(token: &Spanned<Token>) -> Option<(Spanned<BinaryOp>, u8, u8)> {
    let infix = match token.value {
        Token::Or => (BinaryOp::Or, 1, 2),
        Token::And => (BinaryOp::And, 3, 4),
        Token::Eq => (BinaryOp::Eq, 5, 6),
        Token::Ne => (BinaryOp::Ne, 5, 6),
        Token::Lt => (BinaryOp::Lt, 5, 6),
        Token::Le => (BinaryOp::Le, 5, 6),
        Token::Gt => (BinaryOp::Gt, 5, 6),
        Token::Ge => (BinaryOp::Ge, 5, 6),
        Token::Plus => (BinaryOp::Add, 7, 8),
        Token::Minus => (BinaryOp::Sub, 7, 8),
        Token::Star => (BinaryOp::Mul, 9, 10),
        Token::Slash => (BinaryOp::Div, 9, 10),
        _ => return None,
    };

    Some((Spanned::new(infix.0, token.span), infix.1, infix.2))
}

fn lit(value: Value, span: Span) -> Spanned<Expr> {
    Spanned {
        value: Expr::Litteral { value },
        span,
    }
}

fn error(span: Span) -> Spanned<Expr> {
    Spanned {
        value: Expr::Error,
        span,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(src: &str) -> (String, Vec<&'static str>) {
        let mut diags = Diagnostics::from_src(src);
        let expr = ExprParser::new(&Spanned::from_text(src, 0), &mut diags).expr(0);
        let codes = diags.iter().map(|d| d.kind.code()).collect();
        (render(&expr.value), codes)
    }

    fn tree(src: &str) -> String {
        parsed(src).0
    }

    fn codes(src: &str) -> Vec<&'static str> {
        parsed(src).1
    }

    /// Fully parenthesised prefix form, so that a test reads the shape of the
    /// tree instead of its fields.
    fn render(expr: &Expr) -> String {
        match expr {
            Expr::Var { name } => format!("${name}"),
            Expr::Attr { base, attrs } => {
                let attrs = attrs
                    .iter()
                    .map(|s| &*s.value)
                    .collect::<Vec<_>>()
                    .join(".");
                format!("{}.{attrs}", render(&base.value))
            }
            Expr::Litteral { value } => match value {
                Value::Bool(b) => b.to_string(),
                Value::String(s) => format!("{s:?}"),
                Value::Float(f) => f.to_string(),
                Value::Int(i) => i.to_string(),
            },
            Expr::LitteralDict(attrs) => {
                let attrs = attrs
                    .iter()
                    .map(|(key, attr)| format!("{}: {}", key.value, render(&attr.value)))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{{{attrs}}}")
            }
            Expr::Function { name, args } => {
                let mut out = format!("({}", name.value);
                for arg in args {
                    out.push(' ');
                    out.push_str(&render(&arg.value));
                }
                out.push(')');
                out
            }
            Expr::Unary { op, rhs } => format!("({} {})", op.value, render(&rhs.value)),
            Expr::Binary { op, lhs, rhs } => format!(
                "({} {} {})",
                op.value,
                render(&lhs.value),
                render(&rhs.value)
            ),
            Expr::Error => "<error>".to_owned(),
        }
    }

    #[test]
    fn a_value_alone_is_an_expression() {
        assert_eq!(tree("$gold"), "$gold");
        assert_eq!(tree("12"), "12");
        assert_eq!(tree("0.5"), "0.5");
        assert_eq!(tree("true"), "true");
        assert_eq!(tree(r#""Alice""#), r#""Alice""#);
        assert_eq!(tree("gold"), r#""gold""#);
    }

    #[test]
    fn a_product_binds_tighter_than_a_sum() {
        assert_eq!(tree("1 + 2 * 3"), "(+ 1 (* 2 3))");
        assert_eq!(tree("1 * 2 + 3"), "(+ (* 1 2) 3)");
    }

    #[test]
    fn a_comparison_binds_tighter_than_and_which_binds_tighter_than_or() {
        assert_eq!(tree("$a or $b and $c >= 1"), "(or $a (and $b (>= $c 1)))");
    }

    #[test]
    fn operators_of_the_same_priority_group_to_the_left() {
        assert_eq!(tree("1 - 2 - 3"), "(- (- 1 2) 3)");
        assert_eq!(tree("1 / 2 * 3"), "(* (/ 1 2) 3)");
    }

    #[test]
    fn a_unary_operator_binds_tighter_than_every_infix_one() {
        assert_eq!(tree("-2 + 3"), "(+ (- 2) 3)");
        assert_eq!(tree("not $a and $b"), "(and (not $a) $b)");
    }

    #[test]
    fn parentheses_override_the_priorities() {
        assert_eq!(tree("(1 + 2) * 3"), "(* (+ 1 2) 3)");
        assert_eq!(tree("not ($a and $b)"), "(not (and $a $b))");
    }

    #[test]
    fn a_call_holds_its_arguments_in_order() {
        assert_eq!(tree("max(1, $gold)"), "(max 1 $gold)");
        assert_eq!(tree("max(1, min(2, 3))"), "(max 1 (min 2 3))");
    }

    #[test]
    fn a_call_without_argument_is_a_call() {
        assert_eq!(tree("now()"), "(now)");
        assert!(codes("now()").is_empty());
    }

    #[test]
    fn a_call_is_a_value_like_any_other() {
        assert_eq!(tree("max(1, 2) + 3"), "(+ (max 1 2) 3)");
    }

    #[test]
    fn a_call_with_expression_argument() {
        assert_eq!(tree("max(1 + 2, 3)"), "(max (+ 1 2) 3)");
    }

    #[test]
    fn a_var_or_func_with_attr_is_an_attr() {
        assert_eq!(tree("$var.attr"), "$var.attr");
        assert_eq!(tree("$var.attr1.attr2"), "$var.attr1.attr2");
        assert_eq!(tree("$var.attr1.attr2 + 5"), "(+ $var.attr1.attr2 5)");

        assert_eq!(tree("data().attr"), "(data).attr");
        assert_eq!(tree("data().attr1.attr2"), "(data).attr1.attr2");
    }

    #[test]
    fn spans_cover_the_whole_expression() {
        let src = "1 + max(2, 3)";
        let mut diags = Diagnostics::from_src(src);
        let expr = ExprParser::new(&Spanned::from_text(src, 0), &mut diags).expr(0);

        assert_eq!(expr.span, Span { start: 0, end: 13 });
    }

    #[test]
    fn spans_are_shifted_by_the_base_offset() {
        let src = "$gold";
        let mut diags = Diagnostics::from_src(src);
        let expr = ExprParser::new(&Spanned::from_text(src, 12), &mut diags).expr(0);

        assert_eq!(expr.span, Span { start: 12, end: 17 });
    }

    #[test]
    fn error_on_a_missing_operand() {
        assert_eq!(codes("1 +"), ["expected-expression"]);
        assert_eq!(codes(""), ["expected-expression"]);
    }

    #[test]
    fn error_on_an_unclosed_parenthesis() {
        assert_eq!(codes("(1 + 2"), ["unclosed-parenthesis"]);
    }

    #[test]
    fn error_on_a_call_the_line_never_closes() {
        assert_eq!(codes("max(1, 2"), ["unclosed-call"]);
        assert_eq!(codes("max("), ["unclosed-call"]);
    }

    #[test]
    fn error_on_an_argument_that_is_missing() {
        assert_eq!(codes("max(, 1)"), ["expected-expression"]);
        assert_eq!(codes("max(1, )"), ["expected-expression"]);
        assert_eq!(codes("max(1,)"), ["expected-expression"]);
    }

    #[test]
    fn error_on_two_arguments_with_no_separator() {
        assert_eq!(codes("max(1 2)"), ["expected-arg-separator"]);
    }

    #[test]
    fn a_faulty_call_is_reported_once_and_the_rest_of_the_line_is_read() {
        let (tree, codes) = parsed("max(1 2) + 3");

        assert_eq!(codes, ["expected-arg-separator"]);
        assert_eq!(tree, "(+ <error> 3)");
    }

    #[test]
    fn the_closing_parenthesis_skipped_is_the_one_of_the_faulty_call() {
        let (tree, codes) = parsed("max(1 min(2, 3)) + 4");

        assert_eq!(codes, ["expected-arg-separator"]);
        assert_eq!(tree, "(+ <error> 4)");
    }

    #[test]
    fn error_on_attr_on_non_variable() {
        assert_eq!(codes("Alice.attr.attr"), ["attribute-on-non-variable"]);
    }

    #[test]
    fn a_litteral_dict_holds_its_entries_in_order() {
        assert_eq!(tree("{}"), "{}");
        assert_eq!(tree("{hp: 10}"), "{hp: 10}");
        assert_eq!(
            tree(r#"{name: "Leon", age: 1}"#),
            r#"{name: "Leon", age: 1}"#
        );
        assert!(codes("{hp: 10}").is_empty());
    }

    #[test]
    fn a_dict_entry_holds_an_expression() {
        assert_eq!(tree("{hp: $base + 2}"), "{hp: (+ $base 2)}");
        assert_eq!(tree("{hp: max(1, $gold)}"), "{hp: (max 1 $gold)}");
        assert_eq!(tree("{hp: -1}"), "{hp: (- 1)}");
    }

    #[test]
    fn a_dict_nests() {
        assert_eq!(tree("{stats: {hp: 10}}"), "{stats: {hp: 10}}");
        assert_eq!(tree("{a: {b: {c: 1}}, d: 2}"), "{a: {b: {c: 1}}, d: 2}");
    }

    #[test]
    fn a_dict_is_a_value_like_any_other() {
        assert_eq!(tree("{hp: 1} == $data"), "(== {hp: 1} $data)");
        assert_eq!(tree("max({hp: 1}, $data)"), "(max {hp: 1} $data)");
    }

    #[test]
    fn error_on_an_attr_read_on_a_litteral_dict() {
        assert_eq!(codes("{hp: 1}.hp"), ["attribute-on-non-variable"]);
    }

    #[test]
    fn error_on_a_dict_the_line_never_closes() {
        assert_eq!(codes("{hp: 1"), ["unclosed-dict"]);
        assert_eq!(codes("{hp:"), ["unclosed-dict"]);
        assert_eq!(codes("{hp"), ["unclosed-dict"]);
        assert_eq!(codes("{"), ["unclosed-dict"]);
    }

    #[test]
    fn error_on_a_key_that_is_not_a_name() {
        assert_eq!(codes("{1: 2}"), ["expected-key"]);
        assert_eq!(codes("{$hp: 2}"), ["expected-key"]);
    }

    #[test]
    fn error_on_a_missing_colon() {
        assert_eq!(codes("{hp 1}"), ["expected-colon"]);
    }

    #[test]
    fn error_on_a_value_that_is_missing() {
        assert_eq!(codes("{hp: }"), ["expected-expression"]);
        assert_eq!(codes("{hp: , age: 1}"), ["expected-expression"]);
    }

    #[test]
    fn warning_on_a_key_given_twice() {
        let (tree, codes) = parsed("{hp: 1, hp: 2}");

        assert_eq!(codes, ["duplicate-key"]);
        // Both entries are kept by the parser; the map keeps the last one.
        assert_eq!(tree, "{hp: 1, hp: 2}");
    }

    #[test]
    fn a_key_given_twice_is_only_a_warning() {
        let src = "{hp: 1, hp: 2}";
        let mut diags = Diagnostics::from_src(src);
        ExprParser::new(&Spanned::from_text(src, 0), &mut diags).expr(0);

        assert_eq!(diags.errors(), 0);
        assert_eq!(diags.warnings(), 1);
    }

    #[test]
    fn the_span_of_a_dict_covers_its_braces() {
        let src = "{}";
        let mut diags = Diagnostics::from_src(src);
        let expr = ExprParser::new(&Spanned::from_text(src, 0), &mut diags).expr(0);

        assert_eq!(expr.span, Span { start: 0, end: 2 });
    }

    #[test]
    fn error_on_two_entries_with_no_separator() {
        assert_eq!(codes("{hp: 1 age: 2}"), ["expected-arg-separator"]);
    }

    #[test]
    fn a_faulty_dict_is_reported_once_and_the_rest_of_the_line_is_read() {
        let (tree, codes) = parsed("{hp: 1 age: 2} + 3");

        assert_eq!(codes, ["expected-arg-separator"]);
        assert_eq!(tree, "(+ <error> 3)");
    }

    #[test]
    fn the_closing_brace_skipped_is_the_one_of_the_faulty_dict() {
        let (tree, codes) = parsed("{hp: 1 stats: {a: 1}} + 4");

        assert_eq!(codes, ["expected-arg-separator"]);
        assert_eq!(tree, "(+ <error> 4)");
    }
}
