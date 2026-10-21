//! The markup of a text: `Alice: <wave>Hello</wave> <pause=0.5/>there`.
//!
//! Replique gives the markup to the host as it is written, along with
//! the part of the text it covers and leaves the meaning to the host.
//!
//! Three shapes, read in the text of a line or of a choice:
//!
//! - `<name>` opens a markup, and `</name>` closes it.
//! - `<name/>` stands on its own, at a position of the text.
//! - Either can take a value, `<pause=0.5/>`, and attributes,
//!   `<wave speed=10 amplitude=2>`. A value is a [`String`], use `"`
//!   when the text holds blanks: `<link="the old mill">`.
//!
//! A `<` only opens a markup when a letter, a `_` or a `/` directly follows it,
//! so `I <3 you` and `a < b` are text, and `\<` writes a plain `<`.

use std::ops::Range;

/// What is written inside a markup: `wave speed=10` in `<wave speed=10>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkerData {
    /// `wave` in `<wave>`.
    pub name: String,
    /// `0.5` in `<pause=0.5/>`. Kept as it is written.
    pub value: Option<String>,
    /// `speed=10` in `<wave speed=10>`, in the order they are written.
    pub attrs: Vec<(String, String)>,
}

impl MarkerData {
    /// The value of the attribute `key`, if the markup has one.
    pub fn attr(&self, key: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
    }
}

/// A markup of a rendered text, and the part of that text it covers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Markup {
    /// `wave` in `<wave>`.
    pub name: String,
    /// `0.5` in `<pause=0.5/>`. Kept as it is written.
    pub value: Option<String>,
    /// `speed=10` in `<wave speed=10>`, in the order they are written.
    pub attrs: Vec<(String, String)>,
    /// What the markup covers in the text, in bytes, so that `&text[range]` is
    /// that piece of text. Empty for a markup that stands on its own, such as
    /// `<pause/>`, which marks a position.
    pub range: Range<usize>,
}

impl Markup {
    pub(crate) fn new(data: MarkerData, range: Range<usize>) -> Self {
        Self {
            name: data.name,
            value: data.value,
            attrs: data.attrs,
            range,
        }
    }

    /// The value of the attribute `key`, if the markup has one.
    pub fn attr(&self, key: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
    }
}

/// One markup, as it is read in the source.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum MarkupTag {
    /// `<b>`.
    Open(MarkerData),
    /// `</b>`, with the name it closes.
    Close(String),
    /// `<pause/>`.
    Marker(MarkerData),
}

/// Whether the `<` at byte `at` of `text` opens something meant as a markup.
pub(crate) fn is_markup_start(text: &str, at: usize) -> bool {
    let escaped = text[..at].ends_with('\\');
    let after = text[at + 1..].chars().next();

    !escaped && matches!(after, Some(c) if c.is_ascii_alphabetic() || c == '_' || c == '/')
}

/// Reads the markup `text` starts with, and how many bytes it takes. `None`
/// when what follows the `<` does not make one: no `>`, a name missing, an
/// attribute without its value.
pub(crate) fn parse_markup(text: &str) -> Option<(MarkupTag, usize)> {
    let mut reader = Reader { text, at: 0 };
    reader.eat('<').then_some(())?;

    if reader.eat('/') {
        let name = reader.name()?;
        reader.blanks();
        reader.eat('>').then_some(())?;
        return Some((MarkupTag::Close(name), reader.at));
    }

    let name = reader.name()?;
    let value = if reader.eat('=') {
        Some(reader.value()?)
    } else {
        None
    };

    let mut attrs = vec![];
    loop {
        let blank = reader.blanks();

        if reader.eat_str("/>") {
            let data = MarkerData { name, value, attrs };
            return Some((MarkupTag::Marker(data), reader.at));
        }
        if reader.eat('>') {
            let data = MarkerData { name, value, attrs };
            return Some((MarkupTag::Open(data), reader.at));
        }

        // An attribute has to be set apart from what comes before it.
        if !blank {
            return None;
        }
        let key = reader.name()?;
        reader.eat('=').then_some(())?;
        attrs.push((key, reader.value()?));
    }
}

/// Walks a markup one piece at a time.
struct Reader<'a> {
    text: &'a str,
    at: usize,
}

impl<'a> Reader<'a> {
    fn rest(&self) -> &'a str {
        &self.text[self.at..]
    }

    fn eat(&mut self, c: char) -> bool {
        let found = self.rest().starts_with(c);
        if found {
            self.at += c.len_utf8();
        }
        found
    }

    fn eat_str(&mut self, s: &str) -> bool {
        let found = self.rest().starts_with(s);
        if found {
            self.at += s.len();
        }
        found
    }

    /// Skips the blanks, and says whether there were any.
    fn blanks(&mut self) -> bool {
        let rest = self.rest();
        let len = rest.len() - rest.trim_start_matches([' ', '\t']).len();
        self.at += len;
        len > 0
    }

    /// `[A-Za-z_][A-Za-z0-9_-]*`, the name of a markup or of an attribute.
    fn name(&mut self) -> Option<String> {
        let rest = self.rest();
        let first = rest.chars().next()?;
        if !(first.is_ascii_alphabetic() || first == '_') {
            return None;
        }

        let len = rest
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
            .unwrap_or(rest.len());
        self.at += len;
        Some(rest[..len].to_owned())
    }

    /// A word, or a text between quotes in which `\"` writes a quote.
    fn value(&mut self) -> Option<String> {
        if self.eat('"') {
            let mut out = String::new();
            let mut chars = self.rest().char_indices();
            while let Some((at, c)) = chars.next() {
                match c {
                    '"' => {
                        self.at += at + 1;
                        return Some(out);
                    }
                    '\\' => out.push(chars.next()?.1),
                    _ => out.push(c),
                }
            }
            return None;
        }

        let rest = self.rest();
        let len = rest
            .find(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '=' | '/' | '"'))
            .unwrap_or(rest.len());
        if len == 0 {
            return None;
        }
        self.at += len;
        Some(rest[..len].to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data(name: &str, value: Option<&str>, attrs: &[(&str, &str)]) -> MarkerData {
        MarkerData {
            name: name.to_owned(),
            value: value.map(str::to_owned),
            attrs: attrs
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
        }
    }

    fn tag(src: &str) -> Option<MarkupTag> {
        parse_markup(src).map(|(tag, _)| tag)
    }

    #[test]
    fn a_markup_opens_closes_or_stands_alone() {
        assert_eq!(
            tag("<wave>"),
            Some(MarkupTag::Open(data("wave", None, &[])))
        );
        assert_eq!(tag("</wave>"), Some(MarkupTag::Close("wave".to_owned())));
        assert_eq!(
            tag("<pause/>"),
            Some(MarkupTag::Marker(data("pause", None, &[])))
        );
    }

    #[test]
    fn a_markup_takes_a_value_and_attributes() {
        assert_eq!(
            tag("<pause=0.5/>"),
            Some(MarkupTag::Marker(data("pause", Some("0.5"), &[])))
        );
        assert_eq!(
            tag("<wave speed=10 amplitude=2.5>"),
            Some(MarkupTag::Open(data(
                "wave",
                None,
                &[("speed", "10"), ("amplitude", "2.5")]
            )))
        );
        assert_eq!(
            tag("<color=#ff0000 alpha=0.5>"),
            Some(MarkupTag::Open(data(
                "color",
                Some("#ff0000"),
                &[("alpha", "0.5")]
            )))
        );
        assert_eq!(
            tag(r#"<link="the \"old\" mill" kind=place/>"#),
            Some(MarkupTag::Marker(data(
                "link",
                Some(r#"the "old" mill"#),
                &[("kind", "place")]
            )))
        );
    }

    #[test]
    fn blanks_are_allowed_before_the_end() {
        assert_eq!(tag("<b >"), Some(MarkupTag::Open(data("b", None, &[]))));
        assert_eq!(
            tag("<br />"),
            Some(MarkupTag::Marker(data("br", None, &[])))
        );
        assert_eq!(tag("</b >"), Some(MarkupTag::Close("b".to_owned())));
    }

    #[test]
    fn the_length_is_the_markup_alone() {
        assert_eq!(parse_markup("<b>bold</b>").map(|(_, len)| len), Some(3));
        assert_eq!(
            parse_markup("<pause=1/> then").map(|(_, len)| len),
            Some(10)
        );
    }

    #[test]
    fn what_does_not_make_a_markup_is_refused() {
        for src in [
            "<b",
            "<b c",
            "<wave speed>",
            "<wave speed=>",
            "<wave=>",
            r#"<link="never closed>"#,
            "<wave speed=1amplitude=2>",
            "</b x=1>",
            "</>",
        ] {
            assert_eq!(tag(src), None, "{src}");
        }
    }

    #[test]
    fn a_markup_starts_with_a_letter_or_a_slash() {
        let starts = |text: &str| is_markup_start(text, text.find('<').unwrap());

        assert!(starts("<b>"));
        assert!(starts("</b>"));
        assert!(starts("<_x>"));
        assert!(!starts("I <3 you"));
        assert!(!starts("a < b"));
        assert!(!starts("<- back"));
        assert!(!starts(r"\<b>"));
    }

    #[test]
    fn an_attribute_is_read_by_its_key() {
        let markup = data("wave", None, &[("speed", "10")]);

        assert_eq!(markup.attr("speed"), Some("10"));
        assert_eq!(markup.attr("amplitude"), None);
    }
}
