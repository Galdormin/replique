//! Syntax of a `>>` command: `command`, `command()` or `command(1, "two", true)`.
//!
//! Kept apart from the parser because the language server needs the very same
//! rules to paint a command, including one that is still being typed and that
//! the parser therefore rejects.
//!
//! A single left-to-right [`scan`] knows what a string literal and a nested
//! call are; everything else here is a filter over it, so the two cannot
//! disagree on where an argument ends.

use crate::parser::{Span, Spanned};

/// A command line split into its parts, none of them validated.
#[derive(Debug, Clone, PartialEq)]
pub struct CommandParts<'a> {
    /// Everything before the `(`, or the whole payload without one.
    pub name: Spanned<&'a str>,
    /// Arguments between the parentheses, empty for both `f` and `f()`.
    /// Use [`has_parens`](Self::has_parens) to tell those two apart.
    pub args: Vec<Spanned<&'a str>>,
    /// Whether the payload has an argument list at all.
    pub has_parens: bool,
    /// Whether the argument list ends on its matching `)` with nothing but
    /// blanks after it.
    pub closed: bool,
    /// Whatever follows the matching `)`, which belongs to no argument.
    pub trailing: Option<Spanned<&'a str>>,
    /// The string literal left open at the end of the payload, from its
    /// opening quote to the end of the line.
    pub unterminated_string: Option<Span>,
}

/// A character of a command payload, with the state *before* it.
#[derive(Debug, Clone, Copy)]
struct Cursor {
    /// Byte offset of `ch` inside the scanned payload.
    index: usize,
    ch: char,
    /// Nesting of `(`, ignoring those inside a string.
    depth: usize,
    /// Whether a string literal is open.
    in_string: bool,
    /// Whether `ch` is preceded by a backslash inside a string.
    escaped: bool,
}

/// Walk `payload`, keeping track of string literals and nesting.
///
/// This is the only place that knows what a string literal is: `split_command`
/// and [`split_args`] both read the state it exposes rather than looking at the
/// characters themselves.
fn scan(payload: &str) -> impl Iterator<Item = Cursor> + '_ {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    payload.char_indices().map(move |(index, ch)| {
        let cursor = Cursor {
            index,
            ch,
            depth,
            in_string,
            escaped,
        };

        match ch {
            _ if escaped => escaped = false,
            '\\' if in_string => escaped = true,
            '"' => in_string = !in_string,
            // Inside a string nothing else is punctuation.
            _ if in_string => (),
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            _ => (),
        }

        cursor
    })
}

/// Split what follows a `>>` into a name and an argument list.
///
/// Nothing is validated: an unclosed list, a trailing token or an unterminated
/// string are reported through the fields of [`CommandParts`], never by
/// failing, so that a command being typed still splits.
pub fn split_command(cmd: Spanned<&str>) -> CommandParts<'_> {
    let (mut open, mut close, mut quote) = (None, None, None);

    for c in scan(cmd.value) {
        match c.ch {
            // An escaped quote does not open nor close a literal.
            '"' if !c.escaped => quote = if c.in_string { None } else { Some(c.index) },
            '(' if !c.in_string && c.depth == 0 && open.is_none() => open = Some(c.index),
            ')' if !c.in_string && c.depth == 1 && close.is_none() => close = Some(c.index),
            _ => (),
        }
    }

    let unterminated_string =
        quote.map(|i| Span::from_length(cmd.span.start + i, cmd.value.len() - i));

    // Command is `command`, everything is the name.
    let Some(open) = open else {
        return CommandParts {
            name: cmd,
            args: vec![],
            has_parens: false,
            closed: true,
            trailing: None,
            unterminated_string,
        };
    };

    let name = Spanned::from_text(&cmd.value[..open], cmd.span.start);

    // Command is `command()` or `command(1, "two", true)`, possibly unfinished.
    let (end, trailing) = match close {
        Some(close) => {
            let after = &cmd.value[close + 1..];
            let trailing = (!after.trim().is_empty())
                .then(|| Spanned::from_text(after, cmd.span.start + close + 1));
            (close, trailing)
        }
        None => (cmd.value.len(), None),
    };

    let inner = Spanned::from_text(&cmd.value[open + 1..end], cmd.span.start + open + 1);

    CommandParts {
        name,
        args: split_args(inner),
        has_parens: true,
        closed: close.is_some() && trailing.is_none(),
        trailing,
        unterminated_string,
    }
}

/// Splits an argument list on its top level commas, so that a comma inside a
/// string or inside a nested call stays part of its argument.
///
/// An empty list yields no argument, so that `f()` takes none. `args` is
/// expected to be the inside of the parentheses, already trimmed.
///
/// Nothing is validated: an unterminated string swallows the rest of the list,
/// and an unbalanced `)` is simply ignored. Both are reported later, from the
/// spans this returns.
pub fn split_args(args: Spanned<&str>) -> Vec<Spanned<&str>> {
    if args.value.trim().is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    let mut start = 0;

    for c in scan(args.value) {
        if c.ch == ',' && c.depth == 0 && !c.in_string {
            out.push(Spanned::from_text(
                &args.value[start..c.index],
                args.span.start + start,
            ));
            start = c.index + 1;
        }
    }
    out.push(Spanned::from_text(
        &args.value[start..],
        args.span.start + start,
    ));

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(text: &str) -> Spanned<&str> {
        Spanned::from_text(text, 0)
    }

    /// Expected parts of a command whose list is properly closed.
    fn closed<'a>(name: Spanned<&'a str>, args: Vec<Spanned<&'a str>>) -> CommandParts<'a> {
        CommandParts {
            name,
            args,
            has_parens: true,
            closed: true,
            trailing: None,
            unterminated_string: None,
        }
    }

    #[test]
    fn a_payload_without_parentheses_is_all_name() {
        assert_eq!(
            split_command(text("fade_in")),
            CommandParts {
                name: Spanned::from_text("fade_in", 0),
                args: vec![],
                has_parens: false,
                closed: true,
                trailing: None,
                unterminated_string: None,
            }
        );
    }

    /// `f` and `f()` both take no argument, but only one of them was written
    /// with a list: the language server needs to tell them apart.
    #[test]
    fn empty_parentheses_are_kept_apart_from_no_parentheses() {
        let bare = split_command(text("fade_in"));
        let empty = split_command(text("fade_in()"));

        assert_eq!(bare.args, empty.args);
        assert!(!bare.has_parens);
        assert!(empty.has_parens);
        assert!(empty.closed);
    }

    #[test]
    fn a_closed_list_gives_back_what_it_holds() {
        assert_eq!(
            split_command(text("play(1, 2)")),
            closed(
                Spanned::from_text("play", 0),
                vec![Spanned::from_text("1", 5), Spanned::from_text("2", 8)]
            )
        );
        assert_eq!(
            split_command(text("play ( 1 )")),
            closed(
                Spanned::from_text("play", 0),
                vec![Spanned::from_text("1", 7)]
            )
        );
    }

    #[test]
    fn a_nested_call_is_a_single_argument() {
        assert_eq!(
            split_command(text("play(g(1, 2), $x)")),
            closed(
                Spanned::from_text("play", 0),
                vec![
                    Spanned::from_text("g(1, 2)", 5),
                    Spanned::from_text("$x", 14)
                ]
            )
        );
    }

    #[test]
    fn a_missing_parenthesis_leaves_the_list_unclosed() {
        assert_eq!(
            split_command(text("play(1")),
            CommandParts {
                name: Spanned::from_text("play", 0),
                args: vec![Spanned::from_text("1", 5)],
                has_parens: true,
                closed: false,
                trailing: None,
                unterminated_string: None,
            }
        );
    }

    /// The arguments stay valid: only what follows the `)` is at fault.
    #[test]
    fn a_trailing_token_is_reported_apart_from_the_arguments() {
        assert_eq!(
            split_command(text("play(1) toto")),
            CommandParts {
                name: Spanned::from_text("play", 0),
                args: vec![Spanned::from_text("1", 5)],
                has_parens: true,
                closed: false,
                trailing: Some(Spanned::from_text("toto", 8)),
                unterminated_string: None,
            }
        );
        assert_eq!(
            split_command(text("play(1))")),
            CommandParts {
                name: Spanned::from_text("play", 0),
                args: vec![Spanned::from_text("1", 5)],
                has_parens: true,
                closed: false,
                trailing: Some(Spanned::from_text(")", 7)),
                unterminated_string: None,
            }
        );
    }

    /// A `)` inside a string must not be taken for the closing one.
    #[test]
    fn an_unterminated_string_does_not_close_the_list() {
        assert_eq!(
            split_command(text(r#"log("oops)"#)),
            CommandParts {
                name: Spanned::from_text("log", 0),
                args: vec![Spanned::from_text(r#""oops)"#, 4)],
                has_parens: true,
                closed: false,
                trailing: None,
                unterminated_string: Some(Span::new(4, 10)),
            }
        );
    }

    #[test]
    fn parentheses_inside_a_string_are_not_counted() {
        assert_eq!(
            split_command(text(r#"say("(hi)")"#)),
            closed(
                Spanned::from_text("say", 0),
                vec![Spanned::from_text(r#""(hi)""#, 4)]
            )
        );
    }

    #[test]
    fn an_escaped_quote_does_not_end_its_string() {
        assert_eq!(
            split_command(text(r#"f("a\",b", 2)"#)),
            closed(
                Spanned::from_text("f", 0),
                vec![
                    Spanned::from_text(r#""a\",b""#, 2),
                    Spanned::from_text("2", 11)
                ]
            )
        );
    }

    #[test]
    fn an_empty_list_holds_no_argument() {
        assert!(split_args(text("")).is_empty());
        assert!(split_args(text("   ")).is_empty());
    }

    #[test]
    fn a_nested_call_stays_one_argument() {
        assert_eq!(
            split_args(text("g(1, 2), $x")),
            vec![
                Spanned::from_text("g(1, 2)", 0),
                Spanned::from_text("$x", 9)
            ]
        );
        assert_eq!(
            split_args(text("f(g(1, 2)), 3")),
            vec![
                Spanned::from_text("f(g(1, 2))", 0),
                Spanned::from_text("3", 12)
            ]
        );
    }

    #[test]
    fn an_expression_keeps_its_operators() {
        assert_eq!(
            split_args(text(r#"$hp + 1, "a""#)),
            vec![
                Spanned::from_text("$hp + 1", 0),
                Spanned::from_text(r#""a""#, 9)
            ]
        );
    }

    /// An unbalanced `)` must not make the following commas disappear.
    #[test]
    fn an_unbalanced_paren_does_not_swallow_the_list() {
        assert_eq!(
            split_args(text("1), 2")),
            vec![Spanned::from_text("1)", 0), Spanned::from_text("2", 4)]
        );
    }

    #[test]
    fn arguments_are_split_on_their_commas_only() {
        assert_eq!(
            split_args(text("1, 2 ,3")),
            vec![
                Spanned::from_text("1", 0),
                Spanned::from_text("2", 3),
                Spanned::from_text("3", 6)
            ]
        );
        assert_eq!(
            split_args(text(r#""un, deux", 3"#)),
            vec![
                Spanned::from_text(r#""un, deux""#, 0),
                Spanned::from_text("3", 12)
            ]
        );
        assert_eq!(
            split_args(text("1,")),
            vec![Spanned::from_text("1", 0), Spanned::from_text("", 2)]
        );
    }

    /// Every span must slice back to exactly the text it describes.
    #[test]
    fn spans_slice_back_to_their_text() {
        for payload in [
            "fade_in",
            "play(1, 2)",
            "play ( 1 )",
            "play(1) toto",
            r#"log("oops)"#,
            r#"say("(hi)")"#,
            r#"say("héhé", $x + 1)"#,
            "play(g(1, 2), $x)",
        ] {
            let parts = split_command(Spanned::from_text(payload, 0));
            let slice = |s: Spanned<&str>| &payload[s.span.start..s.span.end];

            assert_eq!(slice(parts.name), parts.name.value, "name of {payload:?}");
            for arg in &parts.args {
                assert_eq!(slice(*arg), arg.value, "argument of {payload:?}");
            }
            if let Some(trailing) = parts.trailing {
                assert_eq!(slice(trailing), trailing.value, "trailing of {payload:?}");
            }
        }
    }
}
