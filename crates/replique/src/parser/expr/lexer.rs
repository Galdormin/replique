use logos::Logos;

use crate::parser::{
    Span, Spanned,
    diagnostic::{DiagnosticKind, Diagnostics},
};

/// What no token matched. Reported by [`lex`], which holds the source and can
/// therefore quote the offending text; the variants carry nothing themselves.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum LexError {
    #[default]
    UnknownCharacter,
    /// Digits that do not make a number: `1..2`.
    InvalidNumber,
}

#[derive(Logos, Debug, Clone, PartialEq)]
#[logos(error = LexError)]
#[logos(skip r"[ \t]+")]
pub(crate) enum Token {
    /// Variable name: `$gold`, without its `$`.
    #[regex(r"\$[A-Za-z_][A-Za-z0-9_]*", |lex| lex.slice()[1..].to_owned())]
    Var(String),
    /// Attr of Dictionnary: `.attr`
    #[regex(r"\.[A-Za-z_][A-Za-z0-9_]*", |lex| lex.slice()[1..].to_owned())]
    Attr(String),
    /// Function or command
    #[regex(r"[A-Za-z_][A-Za-z0-9_]*\(", |lex| {let s = lex.slice(); s[..s.len() - 1].to_owned()})]
    Func(String),
    /// Identifier `true`, `false` or a bare word
    #[regex(r"[A-Za-z_][A-Za-z0-9_]*", |lex| lex.slice().to_owned())]
    Ident(String),
    /// Deliberately wider than a number, so that `1..2` or `1.2.3` is a [`LexError::InvalidNumber`]
    /// rather than a number followed by unknown characters.
    #[regex(r"[0-9]+(\.[0-9]*)+", |lex| lex.slice().parse().map_err(|_| LexError::InvalidNumber))]
    Float(f64),
    #[regex(r"[0-9]+", |lex| lex.slice().parse().map_err(|_| LexError::InvalidNumber))]
    Int(i64),
    #[regex(r#""([^"\\]|\\.)*""#, |lex| unescape(&lex.slice()[1..lex.slice().len() - 1]))]
    Str(String),

    #[token("==")]
    Eq,
    #[token("!=")]
    Ne,
    #[token("<")]
    Lt,
    #[token("<=")]
    Le,
    #[token(">")]
    Gt,
    #[token(">=")]
    Ge,
    #[token("and")]
    #[token("&&")]
    And,
    #[token("or")]
    #[token("||")]
    Or,
    #[token("not")]
    #[token("!")]
    Not,
    #[token("+")]
    Plus,
    #[token("-")]
    Minus,
    #[token("*")]
    Star,
    #[token("/")]
    Slash,
    #[token("=")]
    Assign,
    #[token("(")]
    LParen,
    #[token(")")]
    RParen,
    #[token("{")]
    LBrace,
    #[token("}")]
    RBrace,
    #[token(",")]
    Separator,
    #[token(":")]
    Colon,

    /// A string with no closing quote. Kept as a token so that what it holds
    /// is not read again as operators, and turned into a [`Token::Str`] plus a
    /// diagnostic by [`lex`]. Never handed to the parser.
    #[regex(r#""([^"\\]|\\.)*\\?"#, |lex| unescape(&lex.slice()[1..]))]
    UnclosedStr(String),
    /// `$` with no name after it. Reported by [`lex`], never handed to the
    /// parser either.
    #[token("$")]
    BareDollar,
}

impl std::fmt::Display for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Var(name) => write!(f, "${name}"),
            Self::Attr(name) => write!(f, ".{name}"),
            Self::Func(name) => write!(f, "{name}("),
            Self::Ident(name) => f.write_str(name),
            Self::Float(n) => write!(f, "{n}"),
            Self::Int(n) => write!(f, "{n}"),
            Self::Str(s) => write!(f, "{s:?}"),
            Self::Eq => f.write_str("=="),
            Self::Ne => f.write_str("!="),
            Self::Lt => f.write_str("<"),
            Self::Le => f.write_str("<="),
            Self::Gt => f.write_str(">"),
            Self::Ge => f.write_str(">="),
            Self::And => f.write_str("and"),
            Self::Or => f.write_str("or"),
            Self::Not => f.write_str("not"),
            Self::Plus => f.write_str("+"),
            Self::Minus => f.write_str("-"),
            Self::Star => f.write_str("*"),
            Self::Slash => f.write_str("/"),
            Self::Assign => f.write_str("="),
            Self::LParen => f.write_str("("),
            Self::RParen => f.write_str(")"),
            Self::LBrace => f.write_str("{"),
            Self::RBrace => f.write_str("}"),
            Self::Separator => f.write_str(","),
            Self::Colon => f.write_str(":"),
            // Neither reaches the parser; `lex` turns them into diagnostics.
            Self::UnclosedStr(s) => write!(f, "\"{s}"),
            Self::BareDollar => f.write_str("$"),
        }
    }
}

/// Tokens and their spans, both of the same length.
#[derive(Debug, Default)]
pub struct Lexed {
    pub tokens: Vec<Spanned<Token>>,
}

impl Lexed {
    fn push(&mut self, tok: Spanned<Token>) {
        self.tokens.push(tok);
    }
}

/// Reads every token of `src`. What does not lex is reported and skipped: the
/// lexer must not fail any more than the parser does.
pub(crate) fn lex(src: &Spanned<&str>, diags: &mut Diagnostics) -> Lexed {
    let mut out = Lexed::default();
    let offset = src.span.start;

    for (result, range) in Token::lexer(src.value).spanned() {
        let span = Span {
            start: offset + range.start,
            end: offset + range.end,
        };

        match result {
            Ok(Token::UnclosedStr(text)) => {
                diags.push(span, DiagnosticKind::UnterminatedString);
                out.push(Spanned::new(Token::Str(text), span));
            }
            Ok(Token::BareDollar) => diags.push(span, DiagnosticKind::EmptyVariableName),
            Ok(token) => out.push(Spanned::new(token, span)),
            Err(LexError::InvalidNumber) => diags.push(
                span,
                DiagnosticKind::InvalidNumber(src.value[range].to_owned()),
            ),
            Err(LexError::UnknownCharacter) => {
                let c = src.value[range]
                    .chars()
                    .next()
                    .expect("a token is never empty");
                diags.push(span, DiagnosticKind::UnknownExpressionCharacter(c));
            }
        }
    }

    out
}

/// Turns `\\` and `\"` back into the character they stand for. The quotes are
/// already stripped by the lexer. A trailing backslash, which only an
/// unclosed literal can end on, is dropped.
fn unescape(inner: &str) -> String {
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();

    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some(escaped) => out.push(escaped),
                None => break,
            },
            _ => out.push(c),
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use crate::parser::LineIndex;

    use super::*;

    fn lexed(src: &str) -> (Vec<Token>, Vec<&'static str>) {
        let mut diags = Diagnostics::new(LineIndex::new(src));
        let out = lex(&Spanned::from_text(src, 0), &mut diags);
        let codes = diags.iter().map(|d| d.kind.code()).collect();
        let tokens = out.tokens.into_iter().map(|t| t.value).collect();
        (tokens, codes)
    }

    fn toks(src: &str) -> Vec<Token> {
        lexed(src).0
    }

    fn codes(src: &str) -> Vec<&'static str> {
        lexed(src).1
    }

    #[test]
    fn a_variable_drops_its_sigil() {
        assert_eq!(toks("$gold"), [Token::Var("gold".into())]);
    }

    #[test]
    fn words_and_symbols_name_the_same_operators() {
        assert_eq!(toks("and or not"), [Token::And, Token::Or, Token::Not]);
        assert_eq!(toks("&& || !"), [Token::And, Token::Or, Token::Not]);
    }

    #[test]
    fn a_two_character_operator_is_never_split() {
        assert_eq!(toks("=="), [Token::Eq]);
        assert_eq!(toks("!="), [Token::Ne]);
        assert_eq!(toks(">= <="), [Token::Ge, Token::Le]);
        assert_eq!(toks("= > <"), [Token::Assign, Token::Gt, Token::Lt]);
    }

    /// `0` is a number like any other, and a longer match is what tells
    /// `0.5` from it.
    #[test]
    fn a_number_can_be_zero() {
        assert_eq!(toks("0"), [Token::Int(0)]);
        assert_eq!(toks("0.0"), [Token::Float(0.0)]);
        assert_eq!(toks("007"), [Token::Int(7)]);
    }

    #[test]
    fn a_number_keeps_its_decimals() {
        assert_eq!(toks("0.5 12"), [Token::Float(0.5), Token::Int(12)]);
    }

    #[test]
    fn a_minus_is_an_operator_not_part_of_the_number() {
        assert_eq!(toks("-2"), [Token::Minus, Token::Int(2)]);
    }

    #[test]
    fn a_string_is_unescaped() {
        assert_eq!(
            toks(r#""a \" b \\ c""#),
            [Token::Str(r#"a " b \ c"#.into())]
        );
    }

    #[test]
    fn a_command_is_separated() {
        assert_eq!(
            toks("command(Alice, 12.0, $var)"),
            [
                Token::Func("command".into()),
                Token::Ident("Alice".into()),
                Token::Separator,
                Token::Float(12.0),
                Token::Separator,
                Token::Var("var".into()),
                Token::RParen,
            ]
        )
    }

    #[test]
    fn a_var_with_attr_is_separated() {
        assert_eq!(
            toks("$var.attr1.attr2"),
            [
                Token::Var("var".into()),
                Token::Attr("attr1".into()),
                Token::Attr("attr2".into()),
            ]
        )
    }

    #[test]
    fn a_litteral_dict_is_separated() {
        assert_eq!(
            toks("{name: \"Jean\", age: 10}"),
            [
                Token::LBrace,
                Token::Ident("name".into()),
                Token::Colon,
                Token::Str("Jean".into()),
                Token::Separator,
                Token::Ident("age".into()),
                Token::Colon,
                Token::Int(10),
                Token::RBrace,
            ]
        )
    }

    #[test]
    fn a_word_stays_an_identifier() {
        assert_eq!(
            toks("true gold"),
            [Token::Ident("true".into()), Token::Ident("gold".into())]
        );
    }

    #[test]
    fn spans_point_at_the_text_of_their_token() {
        let src = r#"$gold >= 50 and $name == "Alice""#;
        let mut diags = Diagnostics::from_src(src);
        let out = lex(&Spanned::from_text(src, 0), &mut diags);

        assert!(diags.errors() == 0);
        for Spanned { value: token, span } in &out.tokens {
            let text = &src[span.start..span.end];
            let expected = match token {
                Token::Str(s) => format!("{s:?}"),
                other => other.to_string(),
            };
            assert_eq!(text, expected, "span of {token:?}");
        }
    }

    #[test]
    fn spans_are_shifted_by_the_base_offset() {
        let src = "$gold";
        let mut diags = Diagnostics::from_src(src);
        let out = lex(&Spanned::from_text(src, 12), &mut diags);

        assert_eq!(out.tokens[0].span, Span { start: 12, end: 17 });
    }

    #[test]
    fn an_unknown_character_is_reported_and_skipped() {
        let (toks, codes) = lexed("$a # $b");

        assert_eq!(toks, [Token::Var("a".into()), Token::Var("b".into())]);
        assert_eq!(codes, ["unknown-expression-character"]);
    }

    #[test]
    fn error_on_a_variable_without_a_name() {
        assert_eq!(codes("$ + 1"), ["empty-variable-name"]);
    }

    #[test]
    fn error_on_an_unterminated_string() {
        let (toks, codes) = lexed(r#""oups"#);

        assert_eq!(toks, [Token::Str("oups".into())]);
        assert_eq!(codes, ["unterminated-string"]);
    }

    #[test]
    fn error_on_a_shapeless_number() {
        assert_eq!(codes("1..2"), ["invalid-number"]);
    }
}
