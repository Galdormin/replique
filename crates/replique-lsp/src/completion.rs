//! What a document offers to complete, and where each kind of item applies.

use std::collections::{HashMap, HashSet};

use replique::parser::{
    ast::{NodeDecl, StmtKind},
    validation::{RepliqueSchema, TagScope},
};
use tower_lsp_server::ls_types::{CompletionItem, CompletionItemKind, InsertTextFormat};

/// Every name a document knows, sorted by what it names.
#[derive(Debug, Default)]
pub struct Completion {
    pub nodes: HashSet<String>,
    pub characters: HashSet<String>,
    pub commands: HashSet<String>,
    pub tags: HashSet<String>,
    pub tag_scopes: HashMap<String, Vec<String>>,
    pub vars: HashSet<String>,
}

/// What the text before the cursor asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Context<'a> {
    /// After `$`
    Variable,
    /// After `>>`
    Command { needs_space: bool },
    /// After `=>`.
    Node { needs_space: bool },
    /// After `#`, on a line of `speaker`. A line and a choice have none.
    Tag { speaker: Option<&'a str> },
    /// On the first word of a line: a speaker, or a command to prefix with `>>`.
    LineStart,
}

impl Completion {
    pub fn new(nodes: &[NodeDecl]) -> Self {
        let mut completion = Self::default();
        for node in nodes {
            completion.nodes.insert(node.name.value.clone());

            for stmt in node.all_statements() {
                completion.populate(&stmt.kind);
            }
        }

        completion
    }

    /// Replace characters, commands, and tags with the ones in
    /// the schema if the given section is defined.
    pub fn with_schema(mut self, schema: &RepliqueSchema) -> Self {
        if !schema.speakers.is_empty() {
            self.characters = schema.speakers.iter().cloned().collect();
        }

        if !schema.commands.is_empty() {
            self.commands = schema.commands.iter().map(|c| c.name.clone()).collect();
        }

        if !schema.tags.is_empty() {
            self.tags = schema.tags.iter().map(|c| c.name.clone()).collect();
            self.tag_scopes = schema
                .tags
                .iter()
                .filter_map(|tag| match &tag.scope {
                    TagScope::All => None,
                    TagScope::Speakers(speakers) => Some((tag.name.clone(), speakers.clone())),
                })
                .collect();
        }

        self
    }

    fn populate(&mut self, stmt: &StmtKind) {
        match stmt {
            StmtKind::Say {
                speaker: Some(speaker),
                tags,
                ..
            } => {
                self.characters.insert(speaker.value.clone());
                self.tags.extend(tags.iter().map(|t| t.value.name.clone()))
            }
            StmtKind::Say { tags, .. } => {
                self.tags.extend(tags.iter().map(|t| t.value.name.clone()))
            }
            StmtKind::Command { name, .. } => {
                self.commands.insert(name.value.clone());
            }
            StmtKind::Set { name, .. } => {
                self.vars.insert(name.value.clone());
            }
            StmtKind::Choice { choices } => {
                let new_tags = choices
                    .iter()
                    .flat_map(|c| &c.tags)
                    .map(|t| t.value.name.clone());
                self.tags.extend(new_tags);
            }
            _ => (),
        }
    }

    /// Items to offer for `context`.
    pub fn items(&self, context: Context<'_>, snippets: bool) -> Vec<CompletionItem> {
        match context {
            Context::Variable => items(
                &self.vars,
                CompletionItemKind::VARIABLE,
                "variable",
                |name| name.to_string(),
                '0',
            ),
            Context::Command { needs_space } => {
                commands(&self.commands, |name| pad(needs_space, name), snippets, '0')
            }
            Context::Node { needs_space } => items(
                &self.nodes,
                CompletionItemKind::MODULE,
                "node",
                |name| pad(needs_space, name),
                '0',
            ),
            // Only the tags the line can carry: one the schema keeps for other
            // speakers would be reported as soon as it is picked.
            Context::Tag { speaker } => {
                let allowed = self
                    .tags
                    .iter()
                    .filter(|name| match self.tag_scopes.get(*name) {
                        None => true,
                        Some(scope) => speaker.is_some_and(|s| scope.iter().any(|x| x == s)),
                    })
                    .cloned()
                    .collect();

                items(
                    &allowed,
                    CompletionItemKind::VALUE,
                    "tag",
                    |name| name.to_string(),
                    '0',
                )
            }
            // Speakers are what a line usually starts with, so they come first.
            Context::LineStart => {
                let mut out = items(
                    &self.characters,
                    CompletionItemKind::VALUE,
                    "character",
                    |name| format!("{name}: "),
                    '0',
                );
                out.extend(commands(
                    &self.commands,
                    |name| format!(">> {name}"),
                    snippets,
                    '1',
                ));
                out
            }
        }
    }
}

/// One entry per name, in alphabetical order.
fn items(
    names: &HashSet<String>,
    kind: CompletionItemKind,
    detail: &str,
    insert: impl Fn(&str) -> String,
    rank: char,
) -> Vec<CompletionItem> {
    let mut names = names.iter().collect::<Vec<_>>();
    names.sort();

    names
        .into_iter()
        .map(|name| CompletionItem {
            label: name.clone(),
            kind: Some(kind),
            detail: Some(detail.to_string()),
            insert_text: Some(insert(name)),
            sort_text: Some(format!("{rank}{name}")),
            ..Default::default()
        })
        .collect()
}

/// Commands are called: they carry their `()`, with the cursor left between
fn commands(
    names: &HashSet<String>,
    write: impl Fn(&str) -> String,
    snippets: bool,
    rank: char,
) -> Vec<CompletionItem> {
    let mut out = items(
        names,
        CompletionItemKind::FUNCTION,
        "command",
        |name| {
            let call = if snippets {
                format!("{name}($0)")
            } else {
                format!("{name}()")
            };
            write(&call)
        },
        rank,
    );

    if snippets {
        for item in &mut out {
            item.insert_text_format = Some(InsertTextFormat::SNIPPET);
        }
    }

    out
}

/// The blank a marker is missing, when the cursor sits right after it.
fn pad(needs_space: bool, name: &str) -> String {
    if needs_space {
        format!(" {name}")
    } else {
        name.to_string()
    }
}

/// What to complete, given the text of the line before the cursor.
pub fn context(prefix: &str) -> Option<Context<'_>> {
    if let Some(sigil) = prefix.rfind('$') {
        let name = &prefix[sigil + 1..];
        if name.chars().all(|c| c.is_alphanumeric() || c == '_') {
            return Some(Context::Variable);
        }
    }

    let line = prefix.trim_start();

    if let Some(rest) = line.strip_prefix(">>") {
        // `>> play(` is past the name, and into the arguments.
        return (!rest.contains('(')).then_some(Context::Command {
            needs_space: !rest.starts_with(char::is_whitespace),
        });
    }

    if let Some(rest) = line.strip_prefix("=>") {
        return (!rest.trim_start().contains(char::is_whitespace)).then_some(Context::Node {
            needs_space: !rest.starts_with(char::is_whitespace),
        });
    }

    if let Some(tag) = tag_context(line) {
        return Some(tag);
    }

    if line.starts_with(['-', '=', ':', '>', '[', '/']) {
        return None;
    }

    // First word of the line: no blank, and no `:` that would have closed the speaker already.
    (!line.contains(|c: char| c.is_whitespace() || c == ':')).then_some(Context::LineStart)
}

/// The bracketed markers, whose line holds an expression and no text.
const BRACKET_MARKERS: [&str; 7] = [
    "[let",
    "[if",
    "[elif",
    "[else",
    "[while",
    "[break",
    "[continue",
];

/// Whether `text[at..]` opens a tag the way the parser reads one: a `#` with
/// nothing but a blank before it.
fn opens_tag(text: &str, at: usize) -> bool {
    text[..at]
        .chars()
        .next_back()
        .is_none_or(char::is_whitespace)
}

/// A tag is read on a line of dialogue and on a choice, outside the brackets
/// of an inline expression, and its `#` has to open it the way the parser
/// wants. Only the name is completed: past the `:` of `#mood:`, the value is
/// the writer's own.
fn tag_context(line: &str) -> Option<Context<'_>> {
    let hash = line.rfind('#')?;
    let (before, name) = (&line[..hash], &line[hash + 1..]);

    let mut chars = name.chars();
    let named = match chars.next() {
        None => true,
        Some(first) => {
            (first.is_ascii_alphabetic() || first == '_')
                && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        }
    };
    if !named || !opens_tag(line, hash) {
        return None;
    }

    // A comment, and what an unclosed `[` holds, are not text.
    if before.contains("//") || before.rfind('[') > before.rfind(']') {
        return None;
    }

    let is_choice = before.starts_with("->");
    let takes_no_text = [">>", "=>", ":=", "--"]
        .iter()
        .chain(&BRACKET_MARKERS)
        .any(|marker| before.starts_with(marker));
    if takes_no_text && !is_choice {
        return None;
    }
    if is_choice {
        return Some(Context::Tag { speaker: None });
    }

    let speaker = before
        .split_once(':')
        .map(|(speaker, _)| speaker)
        .filter(|speaker| {
            // `Agent #2` is a name: only a `#` a letter follows is a tag.
            !speaker.match_indices('#').any(|(at, _)| {
                opens_tag(speaker, at)
                    && speaker[at + 1..].starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
            })
        })
        .map(str::trim)
        .filter(|speaker| !speaker.is_empty());

    Some(Context::Tag { speaker })
}

#[cfg(test)]
mod tests {
    use super::*;
    use replique::parser::parse;

    const SOURCE: &str = "\
:= start
Alice: Hi!
>> play(\"bell\")
[let $gold = 1]
-> A choice
    Bob: Hidden in a choice
[if $gold > 0]
    >> shake()
[else]
    Caroline: Hidden in an else
=> meeting
---

:= meeting
Alice: Here we are.
---
";

    fn completion() -> Completion {
        Completion::new(&parse(SOURCE).nodes)
    }

    #[test]
    fn collects_names_from_a_loop_and_from_what_it_holds() {
        let src = "\
:= start
[let $gold = 0]
[while $gold < 3]
    [let $turn = $gold]
    David: Hidden in a loop
    >> wait()
    -> Deeper still
        Eve: Hidden in a choice in a loop
---
";
        let c = Completion::new(&parse(src).nodes);

        assert!(c.characters.contains("David") && c.characters.contains("Eve"));
        assert!(c.commands.contains("wait"));
        assert!(c.vars.contains("turn"));
    }

    #[test]
    fn collects_names_from_nested_blocks() {
        let c = completion();

        let sorted = |set: &HashSet<String>| {
            let mut v = set.iter().cloned().collect::<Vec<_>>();
            v.sort();
            v
        };

        assert_eq!(sorted(&c.characters), ["Alice", "Bob", "Caroline"]);
        assert_eq!(sorted(&c.commands), ["play", "shake"]);
        assert_eq!(sorted(&c.vars), ["gold"]);
        assert_eq!(sorted(&c.nodes), ["meeting", "start"]);
    }

    #[test]
    fn variables_win_wherever_the_sigil_is() {
        assert_eq!(context("[if $go"), Some(Context::Variable));
        assert_eq!(context("[let $"), Some(Context::Variable));
        assert_eq!(context(">> play($go"), Some(Context::Variable));
        // The sigil is closed: back to the command.
        assert_eq!(context("[if $gold > "), None);
    }

    #[test]
    fn markers_ask_for_their_own_names() {
        assert_eq!(
            context(">> pl"),
            Some(Context::Command { needs_space: false })
        );
        assert_eq!(
            context("    => "),
            Some(Context::Node { needs_space: false })
        );
        assert_eq!(context(">>"), Some(Context::Command { needs_space: true }));
        assert_eq!(context("=>"), Some(Context::Node { needs_space: true }));
    }

    #[test]
    fn markers_stop_offering_once_they_are_past_their_name() {
        assert_eq!(context(">> play("), None);
        assert_eq!(context("=> meeting "), None);
    }

    #[test]
    fn line_start_is_the_first_word_only() {
        assert_eq!(context(""), Some(Context::LineStart));
        assert_eq!(context("    Ali"), Some(Context::LineStart));
        // A blank, or a `:`, means the speaker is behind us.
        assert_eq!(context("Alice "), None);
        assert_eq!(context("Alice: Hi"), None);
        // Other markers own their line.
        assert_eq!(context("-> a choice"), None);
        assert_eq!(context("--"), None);
        assert_eq!(context("[if"), None);
    }

    #[test]
    fn line_start_offers_speakers_then_commands() {
        let items = completion().items(Context::LineStart, true);

        let shown = items
            .iter()
            .map(|i| (i.label.as_str(), i.insert_text.as_deref().unwrap()))
            .collect::<Vec<_>>();

        assert_eq!(
            shown,
            [
                ("Alice", "Alice: "),
                ("Bob", "Bob: "),
                ("Caroline", "Caroline: "),
                ("play", ">> play($0)"),
                ("shake", ">> shake($0)"),
            ]
        );
    }

    #[test]
    fn a_command_opens_its_parentheses() {
        let c = completion();

        let insert = |snippets| {
            c.items(Context::Command { needs_space: true }, snippets)
                .into_iter()
                .map(|i| i.insert_text.unwrap())
                .collect::<Vec<_>>()
        };

        assert_eq!(insert(true), [" play($0)", " shake($0)"]);
        assert_eq!(insert(false), [" play()", " shake()"]);
    }

    #[test]
    fn a_marker_missing_its_blank_gets_one() {
        let c = completion();

        let insert = |context| {
            c.items(context, true)
                .into_iter()
                .map(|i| i.insert_text.unwrap())
                .collect::<Vec<_>>()
        };

        assert_eq!(
            insert(Context::Node { needs_space: true }),
            [" meeting", " start"]
        );
        assert_eq!(
            insert(Context::Node { needs_space: false }),
            ["meeting", "start"]
        );
    }

    fn tag(speaker: Option<&str>) -> Option<Context<'_>> {
        Some(Context::Tag { speaker })
    }

    #[test]
    fn a_hash_on_a_line_of_dialogue_asks_for_a_tag() {
        assert_eq!(context("Alice: #"), tag(Some("Alice")));
        assert_eq!(context("Alice: #an"), tag(Some("Alice")));
        assert_eq!(context("    Alice: Hi there #mo"), tag(Some("Alice")));
        assert_eq!(context("Alice: #angry Hi #"), tag(Some("Alice")));
        assert_eq!(context("Agent #2: Hi #"), tag(Some("Agent #2")));
    }

    #[test]
    fn a_line_no_one_says_and_a_choice_have_no_speaker() {
        assert_eq!(context("#"), tag(None));
        assert_eq!(context("The rain falls. #am"), tag(None));
        assert_eq!(context("#mood:calm The rain falls. #"), tag(None));
        assert_eq!(context("-> Leave #"), tag(None));
        assert_eq!(context("    -> #ho"), tag(None));
    }

    /// The same rule as the parser: a blank before, a letter after.
    #[test]
    fn a_hash_that_opens_no_tag_asks_for_nothing() {
        assert_eq!(context("Alice: C#"), None);
        assert_eq!(context("Alice: C#sh"), None);
        assert_eq!(context(r"Alice: \#an"), None);
        assert_eq!(context("Alice: We are #1"), None);
        assert_eq!(context("Alice: #été"), None);
        // Past the name: the value is not completed, nor what follows.
        assert_eq!(context("Alice: #mood:an"), None);
        assert_eq!(context("Alice: #angry "), None);
    }

    #[test]
    fn a_hash_outside_a_text_asks_for_nothing() {
        assert_eq!(context("Alice: [upper(\"#a"), None);
        assert_eq!(context("Alice: Hi // #to"), None);
        assert_eq!(context(">> play(\"bell\", #a"), None);
        assert_eq!(context("[if $n == #a"), None);
        assert_eq!(context(":= #a"), None);
        // A jump still asks for a node, whatever is written after it.
        assert_eq!(context("=> #a"), Some(Context::Node { needs_space: false }));
    }

    #[test]
    fn a_tag_is_asked_for_after_a_closed_inline_expression() {
        assert_eq!(context("Alice: Hi [$name] #"), tag(Some("Alice")));
        assert_eq!(context("[$name] waves. #"), tag(None));
    }

    fn offered(c: &Completion, context: Context<'_>) -> Vec<String> {
        c.items(context, true)
            .into_iter()
            .map(|i| i.label)
            .collect()
    }

    #[test]
    fn tags_are_collected_from_every_line_and_choice() {
        let src = "\
:= start
Alice: #angry Hi #sound:a1
#ambiance:rain The rain falls.
-> #hostile Leave
    Bob: Bye #sad
-> Stay
    Bob: Good.
---
";
        let c = Completion::new(&parse(src).nodes);

        assert_eq!(
            offered(&c, Context::Tag { speaker: None }),
            ["ambiance", "angry", "hostile", "sad", "sound"]
        );
    }

    #[test]
    fn the_tags_of_the_schema_replace_the_ones_of_the_file() {
        let schema = RepliqueSchema::from_toml("[tags]\nhappy = {}\ndelay = {}\n").unwrap();
        let src = ":= start\nAlice: #hapy Hi\n---\n";
        let c = Completion::new(&parse(src).nodes).with_schema(&schema);

        assert_eq!(
            offered(
                &c,
                Context::Tag {
                    speaker: Some("Alice")
                }
            ),
            ["delay", "happy"]
        );
    }

    #[test]
    fn a_schema_without_tags_keeps_the_ones_of_the_file() {
        let schema = RepliqueSchema::from_toml("speakers = [\"Alice\"]\n").unwrap();
        let src = ":= start\nAlice: #angry Hi\n---\n";
        let c = Completion::new(&parse(src).nodes).with_schema(&schema);

        assert_eq!(offered(&c, Context::Tag { speaker: None }), ["angry"]);
    }

    /// A tag the schema keeps for some speakers is only offered on their
    /// lines, where picking it raises no warning.
    #[test]
    fn a_scoped_tag_is_offered_to_its_speakers_only() {
        let schema = RepliqueSchema::from_toml(
            "speakers = [\"Robin\", \"Fanny\"]\n\
             [tags]\n\
             happy = { scope = [\"Robin\", \"Fanny\"] }\n\
             sad = { scope = [\"Fanny\"] }\n\
             delay = {}\n",
        )
        .unwrap();
        let c = Completion::new(&[]).with_schema(&schema);

        let on_a_line_of = |speaker| offered(&c, Context::Tag { speaker });

        assert_eq!(on_a_line_of(Some("Fanny")), ["delay", "happy", "sad"]);
        assert_eq!(on_a_line_of(Some("Robin")), ["delay", "happy"]);
        assert_eq!(on_a_line_of(Some("Stranger")), ["delay"]);
        assert_eq!(on_a_line_of(None), ["delay"]);
    }

    #[test]
    fn a_tag_is_inserted_without_its_hash() {
        let schema = RepliqueSchema::from_toml("[tags]\nhappy = {}\n").unwrap();
        let c = Completion::new(&[]).with_schema(&schema);

        let items = c.items(Context::Tag { speaker: None }, true);

        assert_eq!(items[0].insert_text.as_deref(), Some("happy"));
    }
}
