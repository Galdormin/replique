//! The tags of a line: `Alice: #angry I am angry #sound:alice_01`.
//!
//! A tag is what a line says about itself rather than to the player. It is a
//! word of its own, written `#name` or `#name:value`, and it is read at the
//! start or at the end of the text, never in the middle of it: a tag has no
//! place in the sentence, so it does not get one.
//!
//! A `#` only starts a tag when a letter follows it and nothing but a blank
//! comes before. `C#`, `#1` and `\#angry` are text.

use std::{fmt, sync::LazyLock};

use regex::Regex;

use crate::parser::{Span, Spanned};

/// A tag of a line, `#name` or `#name:value`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Tag {
    /// What comes after the `#`: `[A-Za-z_][A-Za-z0-9_]*`.
    pub name: String,
    /// What comes after the `:`, when there is one: `[A-Za-z0-9_.-]+`.
    /// Kept as it is written, so `#volume:0.5` holds the text `0.5`.
    pub value: Option<String>,
}

impl Tag {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: None,
        }
    }

    pub fn with_value(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: Some(value.into()),
        }
    }
}

impl fmt::Display for Tag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.value {
            Some(value) => write!(f, "#{}:{value}", self.name),
            None => write!(f, "#{}", self.name),
        }
    }
}

/// A whole word that is a tag: `#name`, or `#name:value`.
static TAG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^#([A-Za-z_][A-Za-z0-9_]*)(?::([A-Za-z0-9_.-]+))?$").expect("a valid pattern")
});

/// Reads `word` as a tag, when the whole of it is one.
fn parse_tag(word: &str) -> Option<Tag> {
    let found = TAG.captures(word)?;

    Some(Tag {
        name: found[1].to_owned(),
        value: found.get(2).map(|value| value.as_str().to_owned()),
    })
}

/// Whether the `#` at byte `at` of `text` opens something meant as a tag: a
/// letter follows it, and nothing but a blank comes before.
fn is_tag_start(text: &str, at: usize) -> bool {
    let after = text[at + 1..].chars().next();
    let before = text[..at].chars().next_back();

    matches!(after, Some(c) if c.is_ascii_alphabetic() || c == '_')
        && before.is_none_or(char::is_whitespace)
}

/// `range` is looked at inside `text` rather than on its own, so that what
/// comes right before it still counts: the `#` of `[$n]#x` follows a bracket,
/// not a blank.
pub(crate) fn tag_starts(text: &str, range: std::ops::Range<usize>) -> Vec<std::ops::Range<usize>> {
    text[range.clone()]
        .match_indices('#')
        .map(|(at, _)| range.start + at)
        .filter(|&at| is_tag_start(text, at))
        .map(|at| {
            let len = text[at..range.end]
                .find(char::is_whitespace)
                .unwrap_or(range.end - at);
            at..at + len
        })
        .collect()
}

/// Whether `text` holds anything that opens like a tag.
pub(crate) fn holds_tag(text: &str) -> bool {
    !tag_starts(text, 0..text.len()).is_empty()
}

/// Takes the tags off both ends of `text`, and gives what is left in between.
pub(crate) fn split_tags(text: Spanned<&str>) -> (Spanned<&str>, Vec<Spanned<Tag>>) {
    let Spanned { value, span } = text;
    let (mut start, mut end) = (0, value.len());

    let mut tags = vec![];
    loop {
        let rest = &value[start..end];
        let len = rest.find(char::is_whitespace).unwrap_or(rest.len());
        let Some(tag) = parse_tag(&rest[..len]) else {
            break;
        };

        tags.push(Spanned::new(
            tag,
            Span::from_length(span.start + start, len),
        ));
        let after = &rest[len..];
        start += len + after.len() - after.trim_start().len();
    }

    let mut trailing = vec![];
    loop {
        let rest = &value[start..end];
        let word_start = rest
            .char_indices()
            .rev()
            .find(|(_, c)| c.is_whitespace())
            .map_or(0, |(at, c)| at + c.len_utf8());
        let word = &rest[word_start..];
        let Some(tag) = parse_tag(word) else {
            break;
        };

        trailing.push(Spanned::new(
            tag,
            Span::from_length(span.start + start + word_start, word.len()),
        ));
        end = start + rest[..word_start].trim_end().len();
    }
    tags.extend(trailing.into_iter().rev());

    let rest = Spanned::new(
        &value[start..end],
        Span::from_length(span.start + start, end - start),
    );
    (rest, tags)
}

pub(crate) fn unescape(text: &str) -> String {
    text.replace("\\#", "#")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What is left of `src` and its tags, as they are written.
    fn split(src: &str) -> (&str, Vec<String>) {
        let text = Spanned::new(src, Span::from_length(0, src.len()));
        let (rest, tags) = split_tags(text);
        (
            rest.value,
            tags.iter().map(|tag| tag.value.to_string()).collect(),
        )
    }

    fn starts(src: &str) -> Vec<&str> {
        tag_starts(src, 0..src.len())
            .into_iter()
            .map(|range| &src[range])
            .collect()
    }

    #[test]
    fn a_tag_is_a_name_and_maybe_a_value() {
        assert_eq!(parse_tag("#angry"), Some(Tag::new("angry")));
        assert_eq!(
            parse_tag("#sound:alice_01"),
            Some(Tag::with_value("sound", "alice_01"))
        );
        assert_eq!(parse_tag("#_x9"), Some(Tag::new("_x9")));
    }

    #[test]
    fn a_value_can_start_with_a_digit_and_hold_dots_and_dashes() {
        assert_eq!(
            parse_tag("#volume:0.5"),
            Some(Tag::with_value("volume", "0.5"))
        );
        assert_eq!(
            parse_tag("#line:01ab"),
            Some(Tag::with_value("line", "01ab"))
        );
        assert_eq!(
            parse_tag("#voice:alice-01.ogg"),
            Some(Tag::with_value("voice", "alice-01.ogg"))
        );
    }

    #[test]
    fn a_word_that_is_not_wholly_a_tag_is_not_one() {
        for word in [
            "angry", "#", "#1", "#9lives", "#mood:", "#:angry", "#a:b:c", "#angry,", "#a/b",
            "#été", "\\#angry", "##angry",
        ] {
            assert_eq!(parse_tag(word), None, "{word}");
        }
    }

    #[test]
    fn a_tag_is_written_back_the_way_it_was_read() {
        assert_eq!(Tag::new("angry").to_string(), "#angry");
        assert_eq!(Tag::with_value("mood", "sad").to_string(), "#mood:sad");
    }

    #[test]
    fn tags_are_taken_off_both_ends() {
        assert_eq!(
            split("#angry I am angry #sound:alice_01"),
            (
                "I am angry",
                vec!["#angry".into(), "#sound:alice_01".into()]
            )
        );
        assert_eq!(split("#mood:angry Hi"), ("Hi", vec!["#mood:angry".into()]));
        assert_eq!(split("Hi #a #b"), ("Hi", vec!["#a".into(), "#b".into()]));
        assert_eq!(split("#a #b Hi"), ("Hi", vec!["#a".into(), "#b".into()]));
    }

    #[test]
    fn tags_come_out_in_the_order_they_are_written() {
        assert_eq!(
            split("#a #b Hi #c #d").1,
            ["#a", "#b", "#c", "#d"].map(String::from)
        );
    }

    #[test]
    fn a_text_without_a_tag_is_left_alone() {
        assert_eq!(split("I am angry"), ("I am angry", vec![]));
        assert_eq!(split(""), ("", vec![]));
    }

    #[test]
    fn a_text_can_be_nothing_but_tags() {
        assert_eq!(split("#angry"), ("", vec!["#angry".into()]));
        assert_eq!(split("#a   #b"), ("", vec!["#a".into(), "#b".into()]));
    }

    #[test]
    fn a_tag_in_the_middle_stays_in_the_text() {
        assert_eq!(split("I am #angry today"), ("I am #angry today", vec![]));
        assert_eq!(
            split("#a I am #angry today #b"),
            ("I am #angry today", vec!["#a".into(), "#b".into()])
        );
    }

    #[test]
    fn what_only_looks_like_a_tag_stops_the_reading() {
        assert_eq!(split("#1 is the best"), ("#1 is the best", vec![]));
        assert_eq!(split("I love C#"), ("I love C#", vec![]));
        assert_eq!(split("\\#angry Hi"), ("\\#angry Hi", vec![]));
        assert_eq!(split("Hi #mood:"), ("Hi #mood:", vec![]));
    }

    #[test]
    fn what_is_left_between_the_tags_is_trimmed() {
        assert_eq!(split("#a    Hi there    #b").0, "Hi there");
    }

    #[test]
    fn spans_point_at_the_tags_and_at_what_is_left() {
        let src = "#angry  Je suis là  #sound:a1";
        let text = Spanned::new(src, Span::from_length(10, src.len()));

        let (rest, tags) = split_tags(text);

        let at = |span: Span| &src[span.start - 10..span.end - 10];
        assert_eq!(at(rest.span), "Je suis là");
        assert_eq!(at(tags[0].span), "#angry");
        assert_eq!(at(tags[1].span), "#sound:a1");
    }

    #[test]
    fn a_tag_right_against_non_ascii_text_is_read() {
        assert_eq!(split("Ça va été #a"), ("Ça va été", vec!["#a".into()]));
        assert_eq!(split("#a été"), ("été", vec!["#a".into()]));
    }

    #[test]
    fn a_tag_opens_after_a_blank_and_before_a_letter() {
        assert_eq!(starts("I am #angry today"), ["#angry"]);
        assert_eq!(starts("#a and #b:c, then"), ["#a", "#b:c,"]);
        assert_eq!(starts("#mood: oups"), ["#mood:"]);
    }

    #[test]
    fn a_hash_that_opens_no_tag_is_text() {
        for text in ["C# is fine", "We are #1", "a # b", "\\#angry", "x#y", "#"] {
            assert!(starts(text).is_empty(), "{text}");
            assert!(!holds_tag(text), "{text}");
        }
    }

    #[test]
    fn a_range_is_read_with_what_comes_before_it() {
        let text = "[$n]#x and #y";

        assert_eq!(tag_starts(text, 4..text.len()), vec![11..13]);
    }

    #[test]
    fn an_escaped_hash_is_a_hash() {
        assert_eq!(unescape("\\#angry and C\\#"), "#angry and C#");
        assert_eq!(unescape("a \\ b"), "a \\ b");
    }
}
