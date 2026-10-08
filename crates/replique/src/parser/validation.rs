//! Validate the [`Parsed`](super::Parsed) dialogue based on the
//! [`RepliqueSchema`].
//!
//! The schema is what a project says about itself, and what a dialogue cannot
//! know on its own: who can speak, which tags mean something, and which
//! functions and commands the game answers to. It is written in a
//! `replique.toml`:
//!
//! ```toml
//! speakers = ["Robin", "Fanny", "Bertille"]
//!
//! [tags]
//! happy = { scope = ["Robin", "Fanny"] }
//! delay = {}
//!
//! [functions]
//! get_flower = "1-2"
//!
//! [commands]
//! add_scene = "1+"
//! change_mood = "2"
//! ```
//!
//! Every part is optional, and a part left out is not checked: without
//! `speakers`, anyone can speak. A tag without a `scope` is one any line can carry;
//! with one, only the lines of those speakers can. A function or a command is
//! given the number of arguments it takes: `"2"` for exactly two, `"1+"` for
//! one or more, `"1-2"` for one or two.
//!
//! ```
//! use replique::parser::validation::{Arity, RepliqueSchema};
//!
//! let schema: RepliqueSchema = r#"
//!     speakers = ["Robin"]
//!
//!     [commands]
//!     add_scene = "1+"
//! "#
//! .parse()
//! .unwrap();
//!
//! assert_eq!(schema.speakers, ["Robin"]);
//! assert_eq!(schema.commands[0].arity, Arity::AtLeast(1));
//! ```

use std::{collections::BTreeMap, str::FromStr};

use serde::Deserialize;
use thiserror::Error;

pub use crate::builtins::Arity;
use crate::{
    builtins::{self, lookup},
    parser::{
        Spanned, Tag,
        ast::{NodeDecl, StmtKind},
        diagnostic::{DiagnosticKind, Diagnostics},
    },
};

use super::ast::is_valid_ident;

/// What a `replique.toml` contains.
///
/// See [module](self).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RepliqueSchema {
    /// Allowed speakers.
    pub speakers: Vec<String>,
    /// Allowed tags.
    pub tags: Vec<TagSchema>,
    /// Allowed functions.
    pub functions: Vec<CallSchema>,
    /// Allowed commands.
    pub commands: Vec<CallSchema>,
}

/// Which lines can carry a tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagScope {
    /// Allowed for all speakers and no speaker.
    All,
    /// The lines of these speakers only.
    Speakers(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagSchema {
    pub name: String,
    pub scope: TagScope,
}

/// An arity that reads as none of `"2"`, `"1+"` and `"1-2"`.
#[derive(Error, Debug, Clone, PartialEq, Eq)]
#[error("expected a count such as `2`, `1+` or `1-2`")]
pub struct InvalidArity;

/// Reads an arity the way a schema writes it: `"2"` for exactly two
/// arguments, `"1+"` for one or more, `"1-2"` for one or two.
impl FromStr for Arity {
    type Err = InvalidArity;

    fn from_str(s: &str) -> Result<Self, InvalidArity> {
        let count = |text: &str| text.trim().parse::<usize>().map_err(|_| InvalidArity);
        let s = s.trim();

        if let Some(low) = s.strip_suffix('+') {
            return Ok(Arity::AtLeast(count(low)?));
        }
        if let Some((low, high)) = s.split_once('-') {
            let (low, high) = (count(low)?, count(high)?);
            return if low <= high {
                Ok(Arity::Range(low, high))
            } else {
                Err(InvalidArity)
            };
        }

        Ok(Arity::Exact(count(s)?))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallSchema {
    pub name: String,
    pub arity: Arity,
}

/// Why a `replique.toml` could not be read.
#[derive(Error, Debug, Clone, PartialEq)]
pub enum SchemaError {
    /// Not TOML, or TOML of another shape: a key the schema does not have, a
    /// value of the wrong type. The message says where.
    #[error("{0}")]
    Toml(#[from] toml::de::Error),
    #[error("a speaker has an empty name")]
    EmptySpeaker,
    #[error("speaker `{0}` is given more than once")]
    DuplicateSpeaker(String),
    /// A name no dialogue could ever write, such as a tag called `my@tag`.
    #[error(
        "`{name}` in `[{section}]` is not a valid name: expected letters, digits and `_`, not starting with a digit"
    )]
    InvalidName { section: &'static str, name: String },
    #[error(
        "`{name}` in `[{section}]`: `{value}` is not a number of arguments, expected a count such as `2`, `1+` or `1-2`"
    )]
    InvalidArity {
        section: &'static str,
        name: String,
        value: String,
    },
    /// A tag scoped to someone who is not in `speakers`.
    #[error("tag `{tag}` is scoped to `{speaker}`, who is not one of the speakers")]
    UnknownSpeakerInScope { tag: String, speaker: String },
    /// A function have the same name as a builtin
    #[error("function `{0} is already declared as a builtin`")]
    AlreadyBuiltin(String),
}

/// The file as it is written. The names are the keys of the tables there,
/// and move into the entries once read.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSchema {
    #[serde(default)]
    speakers: Vec<String>,
    #[serde(default)]
    tags: BTreeMap<String, RawTag>,
    #[serde(default)]
    functions: BTreeMap<String, RawArity>,
    #[serde(default)]
    commands: BTreeMap<String, RawArity>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTag {
    scope: Option<Vec<String>>,
}

/// `"1+"`, or a bare `2` for the most common case.
#[derive(Deserialize)]
#[serde(untagged)]
enum RawArity {
    Count(usize),
    Text(String),
}

impl RepliqueSchema {
    /// Reads the content of a `replique.toml`.
    pub fn from_toml(src: &str) -> Result<Self, SchemaError> {
        let raw: RawSchema = toml::from_str(src)?;

        let mut speakers: Vec<String> = Vec::with_capacity(raw.speakers.len());
        for speaker in raw.speakers {
            if speaker.trim().is_empty() {
                return Err(SchemaError::EmptySpeaker);
            }
            if speakers.contains(&speaker) {
                return Err(SchemaError::DuplicateSpeaker(speaker));
            }
            speakers.push(speaker);
        }

        let mut tags = Vec::with_capacity(raw.tags.len());
        for (name, tag) in raw.tags {
            check_name("tags", &name)?;

            let scope = match tag.scope {
                None => TagScope::All,
                Some(scope) => {
                    if let Some(unknown) = scope.iter().find(|s| !speakers.contains(s)) {
                        return Err(SchemaError::UnknownSpeakerInScope {
                            tag: name,
                            speaker: unknown.clone(),
                        });
                    }
                    TagScope::Speakers(scope)
                }
            };
            tags.push(TagSchema { name, scope });
        }

        Ok(Self {
            speakers,
            tags,
            functions: calls("functions", raw.functions)?,
            commands: calls("commands", raw.commands)?,
        })
    }

    /// Validate a [`NodeDecl`] based on the data of the Schema
    pub fn validate(&self, node: &NodeDecl, diags: &mut Diagnostics) {
        for stmt in node.all_statements() {
            match &stmt.kind {
                StmtKind::Say { speaker, tags, .. } => {
                    if let Some(speaker) = speaker {
                        self.validate_speaker(speaker, diags);
                    }
                    self.validate_tags(tags, speaker.as_ref(), diags);
                }
                StmtKind::Choice { choices } => {
                    for choice in choices {
                        self.validate_tags(&choice.tags, None, diags);
                    }
                }
                StmtKind::Command { name, args, .. } => {
                    self.validate_command(name, args.len(), diags);
                }
                _ => {}
            }
        }

        if self.functions.is_empty() {
            return;
        }

        for call in node.all_calls() {
            // We ignore builtins functions
            if builtins::lookup(&call.name.value).is_some() {
                continue;
            }

            if let Some(call_schema) = self.functions.iter().find(|f| f.name == call.name.value) {
                if !call_schema.arity.accepts(call.args) {
                    diags.push(
                        call.span,
                        DiagnosticKind::WrongArity {
                            call: call.name.value.clone(),
                            got: call.args,
                            expected: call_schema.arity,
                        },
                    );
                }
            } else {
                diags.push(
                    call.name.span,
                    DiagnosticKind::UnknownFunction(call.name.value.clone()),
                );
            }
        }
    }

    fn validate_speaker(&self, speaker: &Spanned<String>, diags: &mut Diagnostics) {
        if !self.speakers.is_empty() && !self.speakers.contains(&speaker.value) {
            diags.push(
                speaker.span,
                DiagnosticKind::UnknownSpeaker(speaker.value.clone()),
            );
        }
    }

    fn validate_tags(
        &self,
        tags: &[Spanned<Tag>],
        speaker: Option<&Spanned<String>>,
        diags: &mut Diagnostics,
    ) {
        if self.tags.is_empty() {
            return;
        }

        for Spanned { value: tag, span } in tags {
            let Some(schema) = self.tags.iter().find(|schema| schema.name == tag.name) else {
                diags.push(*span, DiagnosticKind::UnknownTag(tag.name.clone()));
                continue;
            };

            if let TagScope::Speakers(scope) = &schema.scope
                && !speaker.is_some_and(|speaker| scope.contains(&speaker.value))
            {
                diags.push(
                    *span,
                    DiagnosticKind::TagOutOfScope {
                        tag: tag.name.clone(),
                        scope: scope.clone(),
                    },
                );
            }
        }
    }

    fn validate_command(&self, name: &Spanned<String>, len: usize, diags: &mut Diagnostics) {
        if self.commands.is_empty() {
            return;
        }

        if let Some(call) = self.commands.iter().find(|c| c.name == name.value) {
            if !call.arity.accepts(len) {
                diags.push(
                    name.span,
                    DiagnosticKind::WrongArity {
                        call: name.value.clone(),
                        got: len,
                        expected: call.arity,
                    },
                );
            }
        } else {
            diags.push(
                name.span,
                DiagnosticKind::UnknownCommand(name.value.clone()),
            );
        }
    }
}

impl FromStr for RepliqueSchema {
    type Err = SchemaError;

    fn from_str(src: &str) -> Result<Self, SchemaError> {
        Self::from_toml(src)
    }
}

fn check_name(section: &'static str, name: &str) -> Result<(), SchemaError> {
    if is_valid_ident(name) {
        if section == "functions" && lookup(name).is_some() {
            Err(SchemaError::AlreadyBuiltin(name.to_string()))
        } else {
            Ok(())
        }
    } else {
        Err(SchemaError::InvalidName {
            section,
            name: name.to_owned(),
        })
    }
}

fn calls(
    section: &'static str,
    raw: BTreeMap<String, RawArity>,
) -> Result<Vec<CallSchema>, SchemaError> {
    raw.into_iter()
        .map(|(name, arity)| {
            check_name(section, &name)?;

            let arity = match arity {
                RawArity::Count(count) => Arity::Exact(count),
                RawArity::Text(text) => text.parse().map_err(|_| SchemaError::InvalidArity {
                    section,
                    name: name.clone(),
                    value: text,
                })?,
            };
            Ok(CallSchema { name, arity })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL: &str = r#"
speakers = [
    "Robin",
    "Fanny",
    "Bertille",
]

[tags]
neutral = { scope = ["Robin", "Fanny"] }
happy = { scope = ["Robin", "Fanny"] }
sad = { scope = ["Fanny"] }
delay = {}

[functions]
get_flower = "1-2"

[commands]
add_scene = "1+"
remove_scene = "1+"
change_mood = "2"
"#;

    fn schema(src: &str) -> RepliqueSchema {
        RepliqueSchema::from_toml(src).unwrap_or_else(|err| panic!("{err}"))
    }

    fn error(src: &str) -> SchemaError {
        RepliqueSchema::from_toml(src).expect_err("the schema is not valid")
    }

    fn call(name: &str, arity: Arity) -> CallSchema {
        CallSchema {
            name: name.to_owned(),
            arity,
        }
    }

    fn tag(name: &str, scope: Option<&[&str]>) -> TagSchema {
        TagSchema {
            name: name.to_owned(),
            scope: match scope {
                None => TagScope::All,
                Some(speakers) => {
                    TagScope::Speakers(speakers.iter().map(|s| (*s).to_owned()).collect())
                }
            },
        }
    }

    #[test]
    fn a_whole_schema_is_read() {
        assert_eq!(
            schema(FULL),
            RepliqueSchema {
                speakers: vec!["Robin".into(), "Fanny".into(), "Bertille".into()],
                tags: vec![
                    tag("delay", None),
                    tag("happy", Some(&["Robin", "Fanny"])),
                    tag("neutral", Some(&["Robin", "Fanny"])),
                    tag("sad", Some(&["Fanny"])),
                ],
                functions: vec![call("get_flower", Arity::Range(1, 2))],
                commands: vec![
                    call("add_scene", Arity::AtLeast(1)),
                    call("change_mood", Arity::Exact(2)),
                    call("remove_scene", Arity::AtLeast(1)),
                ],
            }
        );
    }

    #[test]
    fn every_part_is_optional() {
        assert_eq!(schema(""), RepliqueSchema::default());
        assert_eq!(
            schema("speakers = []\n[tags]\n[functions]\n[commands]\n"),
            RepliqueSchema::default()
        );
        assert_eq!(
            schema("[commands]\nwave = \"0\"\n").commands,
            [call("wave", Arity::Exact(0))]
        );
    }

    #[test]
    fn a_schema_is_read_through_parse_too() {
        let parsed: RepliqueSchema = FULL.parse().unwrap();

        assert_eq!(parsed, schema(FULL));
    }

    #[test]
    fn speakers_keep_the_order_they_are_written_in() {
        assert_eq!(
            schema(r#"speakers = ["Zoé", "Alice", "Le garde"]"#).speakers,
            ["Zoé", "Alice", "Le garde"]
        );
    }

    #[test]
    fn a_tag_without_a_scope_is_for_everyone() {
        let tags = schema("[tags]\ndelay = {}\n").tags;

        assert_eq!(tags, [tag("delay", None)]);
    }

    /// Not the same as no scope at all: nobody is listed, so nobody can.
    #[test]
    fn a_tag_with_an_empty_scope_is_for_no_one() {
        let tags = schema("[tags]\ndelay = { scope = [] }\n").tags;

        assert_eq!(tags, [tag("delay", Some(&[]))]);
    }

    #[test]
    fn a_tag_can_be_written_as_a_table_of_its_own() {
        let tags = schema("speakers = [\"Fanny\"]\n[tags.sad]\nscope = [\"Fanny\"]\n").tags;

        assert_eq!(tags, [tag("sad", Some(&["Fanny"]))]);
    }

    #[test]
    fn an_arity_is_a_count_a_minimum_or_a_range() {
        assert_eq!("2".parse(), Ok(Arity::Exact(2)));
        assert_eq!("0".parse(), Ok(Arity::Exact(0)));
        assert_eq!("1+".parse(), Ok(Arity::AtLeast(1)));
        assert_eq!("0+".parse(), Ok(Arity::AtLeast(0)));
        assert_eq!("1-2".parse(), Ok(Arity::Range(1, 2)));
        assert_eq!("3-3".parse(), Ok(Arity::Range(3, 3)));
        assert_eq!(" 1 - 2 ".parse(), Ok(Arity::Range(1, 2)));
    }

    #[test]
    fn error_on_what_is_not_an_arity() {
        for text in [
            "", "+", "-", "two", "1.5", "-1", "1-", "-2", "2-1", "1+2", "1++", "1-2-3", "1 2",
        ] {
            assert_eq!(text.parse::<Arity>(), Err(InvalidArity), "{text:?}");
        }
    }

    #[test]
    fn an_arity_accepts_the_counts_it_names() {
        let accepted = |arity: Arity| (0..5).filter(|n| arity.accepts(*n)).collect::<Vec<_>>();

        assert_eq!(accepted(Arity::Exact(2)), [2]);
        assert_eq!(accepted(Arity::AtLeast(3)), [3, 4]);
        assert_eq!(accepted(Arity::Range(1, 2)), [1, 2]);
    }

    /// The most common arity can be written without its quotes.
    #[test]
    fn an_exact_arity_can_be_a_bare_number() {
        assert_eq!(
            schema("[commands]\nchange_mood = 2\n").commands,
            [call("change_mood", Arity::Exact(2))]
        );
    }

    #[test]
    fn error_on_an_arity_that_does_not_read() {
        assert_eq!(
            error("[functions]\nget_flower = \"some\"\n"),
            SchemaError::InvalidArity {
                section: "functions",
                name: "get_flower".into(),
                value: "some".into(),
            }
        );
        assert!(matches!(
            error("[commands]\nadd_scene = \"2-1\"\n"),
            SchemaError::InvalidArity {
                section: "commands",
                ..
            }
        ));
    }

    #[test]
    fn error_on_an_arity_of_another_type() {
        for value in ["true", "1.5", "-1", "[1, 2]", "{ min = 1 }"] {
            let src = format!("[commands]\nadd_scene = {value}\n");

            assert!(matches!(error(&src), SchemaError::Toml(_)), "{value}");
        }
    }

    #[test]
    fn error_on_a_tag_scoped_to_someone_who_does_not_speak() {
        assert_eq!(
            error("speakers = [\"Robin\"]\n[tags]\nsad = { scope = [\"Robin\", \"Fany\"] }\n"),
            SchemaError::UnknownSpeakerInScope {
                tag: "sad".into(),
                speaker: "Fany".into(),
            }
        );
        // With no speaker declared, no scope can name one.
        assert!(matches!(
            error("[tags]\nsad = { scope = [\"Fanny\"] }\n"),
            SchemaError::UnknownSpeakerInScope { .. }
        ));
    }

    #[test]
    fn error_on_a_speaker_given_twice_or_left_empty() {
        assert_eq!(
            error(r#"speakers = ["Robin", "Fanny", "Robin"]"#),
            SchemaError::DuplicateSpeaker("Robin".into())
        );
        assert_eq!(
            error(r#"speakers = ["Robin", "  "]"#),
            SchemaError::EmptySpeaker
        );
    }

    /// TOML takes `my-tag` and `2nd` as keys, but no dialogue can write them
    /// as a tag, a function or a command.
    #[test]
    fn error_on_a_name_no_dialogue_can_write() {
        for (src, section) in [
            ("[tags]\nmy-tag = {}\n", "tags"),
            ("[functions]\n2nd = \"1\"\n", "functions"),
            ("[commands]\n\"add scene\" = \"1\"\n", "commands"),
        ] {
            assert!(
                matches!(error(src), SchemaError::InvalidName { section: s, .. } if s == section),
                "{src}"
            );
        }
    }

    #[test]
    fn error_on_a_key_the_schema_does_not_have() {
        for src in [
            "speaker = [\"Robin\"]\n",
            "[tag]\nhappy = {}\n",
            "[tags]\nhappy = { scop = [] }\n",
        ] {
            assert!(matches!(error(src), SchemaError::Toml(_)), "{src}");
        }
    }

    #[test]
    fn error_on_a_name_given_twice() {
        assert!(matches!(
            error("[commands]\nwave = \"1\"\nwave = \"2\"\n"),
            SchemaError::Toml(_)
        ));
    }

    #[test]
    fn error_on_what_is_not_toml() {
        assert!(matches!(
            error("speakers = [\"Robin\""),
            SchemaError::Toml(_)
        ));
        assert!(matches!(
            error("speakers = \"Robin\"\n"),
            SchemaError::Toml(_)
        ));
    }

    /// A TOML error keeps the line it happened on, which is what makes it
    /// usable from a CLI.
    #[test]
    fn a_toml_error_says_where_it_happened() {
        let message = error("speakers = []\n\n[tags]\nhappy = { scop = [] }\n").to_string();

        assert!(message.contains("line 4"), "{message}");
        assert!(message.contains("scop"), "{message}");
    }

    /// Codes of what validating `src` against `schema` reports, and the text
    /// each one points at.
    fn validated(schema: &str, src: &str) -> Vec<(&'static str, String)> {
        let mut parsed = crate::parser::parse(src);
        assert!(parsed.diagnostics.is_empty(), "the dialogue parses cleanly");

        parsed.validate(&self::schema(schema));

        parsed
            .diagnostics
            .iter()
            .map(|d| (d.kind.code(), src[d.span.start..d.span.end].to_owned()))
            .collect()
    }

    const DIALOGUE: &str = ":= start
Robin: Salut.
Fany: Coucou.
Une ligne sans personne.
[if true]
    -> Partir
        Bertile: Au revoir.
    -> Rester
        Fanny: Reste.
---
";

    #[test]
    fn warning_on_a_speaker_the_schema_does_not_name() {
        assert_eq!(
            validated(r#"speakers = ["Robin", "Fanny", "Bertille"]"#, DIALOGUE),
            [
                ("unknown-speaker", "Fany".to_owned()),
                ("unknown-speaker", "Bertile".to_owned()),
            ]
        );
    }

    #[test]
    fn a_dialogue_whose_speakers_are_all_named_is_valid() {
        assert!(
            validated(
                r#"speakers = ["Robin", "Fany", "Bertile", "Fanny"]"#,
                DIALOGUE
            )
            .is_empty()
        );
    }

    /// A schema without `speakers` says nothing about who can speak: it does
    /// not make everyone unknown.
    #[test]
    fn a_schema_without_speakers_checks_no_speaker() {
        assert!(validated("", DIALOGUE).is_empty());
        assert!(validated("speakers = []", DIALOGUE).is_empty());
        assert!(validated("[commands]\nwave = \"0\"\n", DIALOGUE).is_empty());
    }

    #[test]
    fn an_unknown_speaker_is_only_a_warning() {
        let mut parsed = crate::parser::parse(DIALOGUE);

        parsed.validate(&schema(r#"speakers = ["Robin"]"#));

        assert_eq!(parsed.diagnostics.errors(), 0);
        assert!(!parsed.diagnostics.is_empty());
    }

    const TAGS: &str = r#"
speakers = ["Robin", "Fanny"]

[tags]
happy = { scope = ["Robin", "Fanny"] }
sad = { scope = ["Fanny"] }
delay = {}
"#;

    #[test]
    fn a_tag_of_the_schema_is_valid_where_its_scope_allows_it() {
        let src = ":= start
Robin: #happy Salut #delay:2
Fanny: #sad Bof. #happy
#delay Une ligne sans personne.
-> #delay Partir
    Fanny: Au revoir. #sad
-> Rester
    Robin: Reste.
---
";

        assert!(validated(TAGS, src).is_empty());
    }

    #[test]
    fn warning_on_a_tag_the_schema_does_not_name() {
        let src = ":= start
Robin: #hapy Salut #delay
-> #dellay Partir
    Fanny: Au revoir. #Sad
-> Rester
    Robin: Reste.
---
";

        assert_eq!(
            validated(TAGS, src),
            [
                ("unknown-tag", "#hapy".to_owned()),
                ("unknown-tag", "#dellay".to_owned()),
                ("unknown-tag", "#Sad".to_owned()),
            ]
        );
    }

    #[test]
    fn warning_on_a_tag_on_a_line_of_someone_out_of_its_scope() {
        let src = ":= start\nRobin: #sad Bof. #happy\n---\n";

        assert_eq!(
            validated(TAGS, src),
            [("tag-out-of-scope", "#sad".to_owned())]
        );
    }

    /// A scoped tag belongs to its speakers: a line no one says and a choice
    /// have none to show.
    #[test]
    fn warning_on_a_scoped_tag_where_no_one_speaks() {
        let src = ":= start
#happy Une ligne sans personne.
-> #sad Partir
    Fanny: Au revoir.
-> Rester
    Fanny: Reste.
---
";

        assert_eq!(
            validated(TAGS, src),
            [
                ("tag-out-of-scope", "#happy".to_owned()),
                ("tag-out-of-scope", "#sad".to_owned()),
            ]
        );
    }

    /// The scope is checked against who speaks, known to the schema or not:
    /// the speaker gets its own warning.
    #[test]
    fn an_unknown_speaker_is_out_of_every_scope() {
        let src = ":= start\nFany: #sad Bof. #delay\n---\n";

        assert_eq!(
            validated(TAGS, src),
            [
                ("unknown-speaker", "Fany".to_owned()),
                ("tag-out-of-scope", "#sad".to_owned()),
            ]
        );
    }

    #[test]
    fn a_tag_scoped_to_no_one_is_out_of_scope_everywhere() {
        let schema = "speakers = [\"Robin\"]\n[tags]\nold = { scope = [] }\n";
        let src = ":= start\nRobin: #old Salut.\n#old Personne.\n---\n";

        assert_eq!(
            validated(schema, src),
            [
                ("tag-out-of-scope", "#old".to_owned()),
                ("tag-out-of-scope", "#old".to_owned()),
            ]
        );
    }

    #[test]
    fn tags_are_checked_in_every_block() {
        let src = ":= start
[if true]
    [while false]
        -> un
            Robin: #nope Salut.
        -> #nada deux
            Robin: Salut.
---
";

        assert_eq!(
            validated(TAGS, src),
            [
                ("unknown-tag", "#nada".to_owned()),
                ("unknown-tag", "#nope".to_owned()),
            ]
        );
    }

    /// A schema without `[tags]` says nothing about tags: it does not make
    /// every one of them unknown.
    #[test]
    fn a_schema_without_tags_checks_no_tag() {
        let src = ":= start\nRobin: #anything Salut #goes:1\n---\n";

        assert!(validated("", src).is_empty());
        assert!(validated("speakers = [\"Robin\"]\n[tags]\n", src).is_empty());
    }

    #[test]
    fn the_value_of_a_tag_is_not_checked() {
        let src = ":= start\nRobin: #delay Salut.\nRobin: #delay:2 Salut.\nRobin: #delay:x.y Salut.\n---\n";

        assert!(validated(TAGS, src).is_empty());
    }

    #[test]
    fn a_tag_warning_is_only_a_warning_and_says_who_the_tag_is_for() {
        let src = ":= start\nRobin: #sad Bof. #nope\n---\n";
        let mut parsed = crate::parser::parse(src);

        parsed.validate(&schema(TAGS));

        assert_eq!(parsed.diagnostics.errors(), 0);
        let messages: Vec<_> = parsed
            .diagnostics
            .iter()
            .map(|d| d.kind.to_string())
            .collect();
        assert_eq!(
            messages,
            [
                "tag `#sad` is only for the lines of `Fanny`",
                "tag `#nope` is unknown"
            ]
        );
    }

    const COMMANDS: &str = r#"
[commands]
add_scene = "1+"
change_mood = "2"
fade = "0-1"
wave = "0"
"#;

    #[test]
    fn a_command_called_the_way_the_schema_says_is_valid() {
        let src = ":= start
>> add_scene(Alice)
>> add_scene(Alice, Bob, Caroline)
>> change_mood(Alice, happy)
>> fade()
>> fade(0.5)
>> wave()
>> await wave()
---
";

        assert!(validated(COMMANDS, src).is_empty());
    }

    #[test]
    fn warning_on_a_command_the_schema_does_not_name() {
        let src = ":= start\n>> add_scen(Alice)\n>> await Wave()\n>> wave()\n---\n";

        assert_eq!(
            validated(COMMANDS, src),
            [
                ("unknown-command", "add_scen(".to_owned()),
                ("unknown-command", "Wave(".to_owned()),
            ]
        );
    }

    #[test]
    fn warning_on_a_command_given_the_wrong_number_of_arguments() {
        let src = ":= start
>> add_scene()
>> change_mood(Alice)
>> change_mood(Alice, happy, 2)
>> fade(1, 2)
>> await wave(now)
---
";

        assert_eq!(
            validated(COMMANDS, src),
            [
                ("wrong-arity", "add_scene(".to_owned()),
                ("wrong-arity", "change_mood(".to_owned()),
                ("wrong-arity", "change_mood(".to_owned()),
                ("wrong-arity", "fade(".to_owned()),
                ("wrong-arity", "wave(".to_owned()),
            ]
        );
    }

    #[test]
    fn the_arity_warning_says_what_was_expected_and_what_was_given() {
        let src =
            ":= start\n>> add_scene()\n>> change_mood(Alice)\n>> fade(1, 2)\n>> wave(1)\n---\n";
        let mut parsed = crate::parser::parse(src);

        parsed.validate(&schema(COMMANDS));

        let messages: Vec<_> = parsed
            .diagnostics
            .iter()
            .map(|d| d.kind.to_string())
            .collect();
        assert_eq!(
            messages,
            [
                "`add_scene` expects at least 1 argument, got 0",
                "`change_mood` expects 2 arguments, got 1",
                "`fade` expects 0 to 1 arguments, got 2",
                "`wave` expects 0 arguments, got 1",
            ]
        );
        assert_eq!(parsed.diagnostics.errors(), 0);
    }

    /// An argument counts as one whatever it is made of.
    #[test]
    fn an_argument_is_counted_once_whatever_it_holds() {
        let src = ":= start
[let $a = 1]
>> change_mood({name: Alice, mood: happy}, max($a, 2) + 1)
---
";

        assert!(validated(COMMANDS, src).is_empty());
    }

    #[test]
    fn commands_are_checked_in_every_block() {
        let src = ":= start
[if true]
    [while false]
        -> un
            >> nope()
        -> deux
            >> wave(1)
---
";

        assert_eq!(
            validated(COMMANDS, src),
            [
                ("unknown-command", "nope(".to_owned()),
                ("wrong-arity", "wave(".to_owned()),
            ]
        );
    }

    /// A schema without `[commands]` says nothing about commands: it does not
    /// make every one of them unknown.
    #[test]
    fn a_schema_without_commands_checks_no_command() {
        let src = ":= start\n>> anything(1, 2, 3)\n---\n";

        assert!(validated("", src).is_empty());
        assert!(validated("speakers = [\"Robin\"]\n[commands]\n", src).is_empty());
    }

    /// Functions have their own section: a command is not looked up there.
    #[test]
    fn a_function_of_the_schema_is_not_a_command() {
        let schema = "[functions]\nget_flower = \"1\"\n[commands]\nwave = \"0\"\n";
        let src = ":= start\n>> get_flower(rose)\n---\n";

        assert_eq!(
            validated(schema, src),
            [("unknown-command", "get_flower(".to_owned())]
        );
    }

    const FUNCTIONS: &str = "[functions]\ngold = \"0\"\nget_flower = \"1-2\"\n";

    /// A builtin is known to every schema, and is not looked up in it.
    #[test]
    fn a_function_called_the_way_the_schema_says_is_valid() {
        let src = ":= start
[let $n = gold() + len(get_flower(rose))]
Alice: [upper(get_flower(rose, red))] for [gold()] coins.
---
";

        assert!(validated(FUNCTIONS, src).is_empty());
    }

    #[test]
    fn warning_on_a_function_unknown_or_given_the_wrong_number_of_arguments() {
        let src = ":= start
[if gld() > 1]
    Alice: [get_flower(gold(1))] and [get_flower(a, b, c)]
---
";

        assert_eq!(
            validated(FUNCTIONS, src),
            [
                ("unknown-function", "gld(".to_owned()),
                ("wrong-arity", "gold(1)".to_owned()),
                ("wrong-arity", "get_flower(a, b, c)".to_owned()),
            ]
        );
    }

    /// A schema without `[functions]` says nothing about functions: it does
    /// not make every one of them unknown.
    #[test]
    fn a_schema_without_functions_checks_no_function() {
        let src = ":= start\nAlice: [anything(1, 2, 3)]\n---\n";

        assert!(validated("", src).is_empty());
        assert!(validated("[commands]\nwave = \"0\"\n[functions]\n", src).is_empty());
    }
}
