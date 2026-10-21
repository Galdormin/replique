//! Reading a text through its markups.
//!
//! A [`DialogueLine`] or a [`DialogueChoice`] comes with its plain text and
//! its [`Markup`]s, each one saying which part of the text it covers.
//!
//! [`DialogueLine::pieces`] and [`DialogueChoice::pieces`] do the cutting, and
//! leave the rest to the game:
//!
//! ```
//! # use bevy_replique::prelude::*;
//! # use bevy_replique::markup::pieces;
//! # use replique::RepliqueFile;
//! # use replique::vm::{DialogueEvent, DialogueVm};
//! # let file = RepliqueFile::from_source(
//! #     ":= start\nAlice: <b>Hi</b> there<pause=0.5/>, <b><i>Bob</i></b>.\n---\n",
//! # );
//! # let mut vm = DialogueVm::default();
//! # let DialogueEvent::Say { text, markups, .. } =
//! #     vm.start(file.dialogue.unwrap(), "start").unwrap()
//! # else {
//! #     unreachable!()
//! # };
//! // `Alice: <b>Hi</b> there<pause=0.5/>, <b><i>Bob</i></b>.`
//! for piece in pieces(&text, &markups) {
//!     match piece {
//!         Piece::Text(segment) => {
//!             let bold = segment.has("b");
//!             let italic = segment.has("i");
//!             // Draw `segment.text` in bold, in italic, both or neither.
//! #           let _ = (bold, italic);
//!         }
//!         Piece::Marker(marker) => {
//!             // `<pause=0.5/>`, between `Hi there` and `, Bob.`
//! #           assert_eq!(marker.name, "pause");
//!         }
//!     }
//! }
//! ```
//!
//! [`DialogueLine`]: crate::message::DialogueLine
//! [`DialogueChoice`]: crate::message::DialogueChoice
//! [`DialogueLine::pieces`]: crate::message::DialogueLine::pieces
//! [`DialogueChoice::pieces`]: crate::message::DialogueChoice::pieces

use std::ops::Range;

use replique::dialogue::Markup;

/// A run of text with the same markups over all of it.
#[derive(Debug, Clone, PartialEq)]
pub struct Segment<'a> {
    /// The piece of text, without its markups.
    pub text: &'a str,
    /// Where [`text`](Self::text) is in the whole text, in bytes.
    pub range: Range<usize>,
    /// The markups that cover the whole run, the outermost first: in
    /// `<b><i>Bob</i></b>`, `b` then `i`.
    pub markups: Vec<&'a Markup>,
}

impl Segment<'_> {
    /// Whether the run is under a markup of that name: `segment.has("b")`.
    pub fn has(&self, name: &str) -> bool {
        self.markups.iter().any(|markup| markup.name == name)
    }

    /// The innermost markup of that name over the run, to read its value or
    /// its attributes: the colour of `<color=red>`.
    pub fn get(&self, name: &str) -> Option<&Markup> {
        self.markups
            .iter()
            .rev()
            .find(|markup| markup.name == name)
            .copied()
    }
}

/// What reading a text comes across, in the order.
#[derive(Debug, Clone, PartialEq)]
pub enum Piece<'a> {
    /// A run of text, and the markups over it.
    Text(Segment<'a>),
    /// A markup that covers no text, at the position it stands: `<pause/>`.
    ///
    /// A markup opened and closed around nothing, `<b></b>`, covers no text
    /// either and comes here too.
    Marker(&'a Markup),
}

/// Cuts `text` wherever one of `markups` starts or ends, and gives the runs of
/// text and the markups standing on their own in the order they come.
///
/// Every byte of `text` is in exactly one [`Segment`], so writing the
/// segments one after the other gives `text` back. A marker at a position
/// where a run starts comes before that run.
pub fn pieces<'a>(text: &'a str, markups: &'a [Markup]) -> Vec<Piece<'a>> {
    let (markers, spans): (Vec<&Markup>, Vec<&Markup>) =
        markups.iter().partition(|markup| markup.range.is_empty());

    // Every position where something starts, ends or stands.
    let mut cuts = vec![0, text.len()];
    for markup in markups {
        cuts.extend([markup.range.start, markup.range.end]);
    }
    cuts.sort_unstable();
    cuts.dedup();

    let mut out = vec![];
    for (at, &start) in cuts.iter().enumerate() {
        out.extend(
            markers
                .iter()
                .filter(|marker| marker.range.start == start)
                .map(|marker| Piece::Marker(marker)),
        );

        let Some(&end) = cuts.get(at + 1) else {
            break;
        };
        out.push(Piece::Text(Segment {
            text: &text[start..end],
            range: start..end,
            // `markups` is in the order they are opened, which puts an
            // outer markup before the ones nested in it.
            markups: spans
                .iter()
                .filter(|markup| markup.range.start <= start && end <= markup.range.end)
                .copied()
                .collect(),
        }));
    }

    out
}

#[cfg(test)]
mod tests {
    use replique::{
        RepliqueFile,
        vm::{DialogueEvent, DialogueVm},
    };

    use super::*;

    /// The text and the markups of `line`, as a dialogue gives them.
    fn rendered(line: &str) -> (String, Vec<Markup>) {
        let file = RepliqueFile::from_source(format!(":= start\n{line}\n---\n"));
        assert!(!file.has_errors(), "{line}");

        let mut vm = DialogueVm::default();
        let DialogueEvent::Say { text, markups, .. } =
            vm.start(file.dialogue.unwrap(), "start").unwrap()
        else {
            panic!("expected a line");
        };
        (text, markups)
    }

    /// Each piece of `line` as `"text" b i` or `<marker>`.
    fn read(line: &str) -> Vec<String> {
        let (text, markups) = rendered(line);

        pieces(&text, &markups)
            .into_iter()
            .map(|piece| match piece {
                Piece::Text(segment) => {
                    let names: String = segment
                        .markups
                        .iter()
                        .map(|markup| format!(" {}", markup.name))
                        .collect();
                    format!("{:?}{names}", segment.text)
                }
                Piece::Marker(marker) => format!("<{}>", marker.name),
            })
            .collect()
    }

    #[test]
    fn a_text_without_markup_is_one_segment() {
        assert_eq!(read("Alice: Hello there."), [r#""Hello there.""#]);
    }

    #[test]
    fn a_text_is_cut_where_a_markup_starts_and_ends() {
        assert_eq!(
            read("Alice: Hi <b>there</b> you"),
            [r#""Hi ""#, r#""there" b"#, r#"" you""#]
        );
    }

    #[test]
    fn nested_markups_come_outermost_first() {
        assert_eq!(
            read("Alice: <b>Hi <i>Bob</i></b>!"),
            [r#""Hi " b"#, r#""Bob" b i"#, r#""!""#]
        );
    }

    #[test]
    fn overlapping_runs_get_every_markup_over_them() {
        assert_eq!(
            read("Alice: <b>a<i>b</i></b><i>c</i>"),
            [r#""a" b"#, r#""b" b i"#, r#""c" i"#]
        );
    }

    #[test]
    fn a_marker_stands_where_it_is_written() {
        assert_eq!(
            read("Alice: <pause/>Hi<pause=1/> there<end/>"),
            ["<pause>", r#""Hi""#, "<pause>", r#"" there""#, "<end>"]
        );
    }

    #[test]
    fn a_marker_inside_a_markup_cuts_it_in_two() {
        assert_eq!(
            read("Alice: <b>Hi<pause/> there</b>"),
            [r#""Hi" b"#, "<pause>", r#"" there" b"#]
        );
    }

    #[test]
    fn a_markup_around_nothing_is_a_marker() {
        assert_eq!(read("Alice: Hi<b></b>!"), [r#""Hi""#, "<b>", r#""!""#]);
    }

    #[test]
    fn the_segments_give_the_text_back() {
        let (text, markups) = rendered("Alice: <b>Léa</b> a <i>dit</i><pause/> « <b>bonjour</b> »");

        let joined: String = pieces(&text, &markups)
            .into_iter()
            .filter_map(|piece| match piece {
                Piece::Text(segment) => {
                    assert_eq!(&text[segment.range.clone()], segment.text);
                    Some(segment.text)
                }
                Piece::Marker(_) => None,
            })
            .collect();

        assert_eq!(joined, text);
    }

    #[test]
    fn a_segment_reads_the_innermost_markup_of_a_name() {
        let (text, markups) = rendered("Alice: <color=red>a <color=blue>b</color></color>");
        let pieces = pieces(&text, &markups);

        let Piece::Text(inner) = &pieces[1] else {
            panic!("expected a segment");
        };
        assert_eq!(inner.text, "b");
        assert!(inner.has("color"));
        assert!(!inner.has("b"));
        assert_eq!(
            inner.get("color").and_then(|m| m.value.as_deref()),
            Some("blue")
        );
    }
}
