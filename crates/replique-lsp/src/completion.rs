//! What a document offers to complete, and where each kind of item applies.

use std::collections::HashSet;

use replique::parser::ast::{NodeDecl, Stmt, StmtKind};
use tower_lsp_server::ls_types::{CompletionItem, CompletionItemKind, InsertTextFormat};

/// Every name a document knows, sorted by what it names.
#[derive(Debug, Default)]
pub struct Completion {
    pub nodes: HashSet<String>,
    pub characters: HashSet<String>,
    pub commands: HashSet<String>,
    pub vars: HashSet<String>,
}

/// What the text before the cursor asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Context {
    /// After `$`
    Variable,
    /// After `>>`
    Command { needs_space: bool },
    /// After `=>`.
    Node { needs_space: bool },
    /// On the first word of a line: a speaker, or a command to prefix with `>>`.
    LineStart,
}

impl Completion {
    pub fn new(nodes: &[NodeDecl]) -> Self {
        let mut completion = Self::default();
        for node in nodes {
            completion.nodes.insert(node.name.value.clone());

            for Stmt { kind, .. } in &node.body {
                completion.populate(kind);
            }
        }

        completion
    }

    fn populate(&mut self, stmt: &StmtKind) {
        match stmt {
            StmtKind::Say {
                speaker: Some(speaker),
                ..
            } => {
                self.characters.insert(speaker.value.clone());
            }
            StmtKind::Command { name, .. } => {
                self.commands.insert(name.value.clone());
            }
            StmtKind::Set { name, .. } => {
                self.vars.insert(name.value.clone());
            }
            StmtKind::If {
                branches,
                otherwise,
            } => {
                let stmts = otherwise
                    .as_deref()
                    .unwrap_or(&[])
                    .iter()
                    .chain(branches.iter().flat_map(|b| &b.body));
                for stmt in stmts {
                    self.populate(&stmt.kind);
                }
            }
            StmtKind::Choice { choices } => {
                for stmt in choices.iter().flat_map(|c| &c.body) {
                    self.populate(&stmt.kind);
                }
            }
            _ => (),
        }
    }

    /// Items to offer for `context`.
    pub fn items(&self, context: Context, snippets: bool) -> Vec<CompletionItem> {
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
pub fn context(prefix: &str) -> Option<Context> {
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

    if line.starts_with(['-', '=', ':', '>', '[', '/']) {
        return None;
    }

    // First word of the line: no blank, and no `:` that would have closed the speaker already.
    (!line.contains(|c: char| c.is_whitespace() || c == ':')).then_some(Context::LineStart)
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
}
