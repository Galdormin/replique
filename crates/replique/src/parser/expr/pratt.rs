use crate::parser::{
    Span, Spanned,
    ast::Value,
    diagnostic::{DiagnosticKind, Diagnostics},
    expr::{
        BinaryOp, Expr, UnaryOp,
        lexer::{Lexed, Token, lex},
    },
};

/// Binding power of the unary operators, above every infix one.
const PREFIX_PRIO: u8 = 11;

/// Reads the expression `src` holds, and checks the types of its operators.
pub(crate) fn parse(src: &Spanned<&str>, diags: &mut Diagnostics) -> Spanned<Expr> {
    let expr = ExprParser::new(src, diags).expr(0);
    expr.value.is_type_valid(diags);
    expr
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

    /// Span of the next token, without consuming it.
    fn peek_span(&self) -> Option<Span> {
        self.peek().map(|tok| tok.span)
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
        let Some(Spanned { value, span }) = self.peek().cloned() else {
            self.diags
                .push(self.end_span, DiagnosticKind::ExpectedExpression);
            return error(self.end_span);
        };

        self.bump();

        match value {
            Token::Var(name) => Spanned {
                value: Expr::Var { name },
                span,
            },
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
                if matches!(
                    self.peek(),
                    Some(&Spanned {
                        value: Token::RParen,
                        ..
                    })
                ) {
                    self.bump();
                } else {
                    self.diags.push(span, DiagnosticKind::UnclosedParenthesis);
                }
                expr
            }
            _ => {
                self.diags.push(span, DiagnosticKind::ExpectedExpression);
                error(span)
            }
        }
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
                    value: Token::RParen | Token::ArgSeparator,
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
            if self
                .eat_if(|tok| matches!(tok, Token::ArgSeparator))
                .is_none()
            {
                return match self.peek_span() {
                    Some(span) => {
                        self.diags.push(span, DiagnosticKind::ExpectedArgSeparator);
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

    /// Gives up on a call whose fault is already reported: what is left of it
    /// is skipped so that the caller reads the rest of the line as if the call
    /// had been well formed, and the whole call stands as one [`Expr::Error`].
    fn abandon_call(&mut self, name_span: Span) -> Spanned<Expr> {
        let end = self.skip_to_call_end().unwrap_or(name_span);
        error(name_span.join(end))
    }

    /// Reads up to and including the `)` closing the call being parsed, and
    /// gives back its span. Calls and parentheses opened along the way are
    /// counted, so their own `)` does not end the skip. A call that the line
    /// never closes stops at the end of the tokens, and gives back nothing.
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
            Expr::Litteral { value } => match value {
                Value::Bool(b) => b.to_string(),
                Value::String(s) => format!("{s:?}"),
                Value::Float(f) => f.to_string(),
                Value::Int(i) => i.to_string(),
            },
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
}
