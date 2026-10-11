//! A pure Rust dialogue system.
//!
//! Replique reads dialogues written in plain text, compiles them, and runs them
//! on a small virtual machine. It knows nothing about any engine: the VM hands
//! out events (*say this line*, *offer these choices*, *run this command*)
//! and waits for the caller to answer. [`bevy_replique`] is the Bevy plugin
//! built on top of this crate.
//!
//! [`bevy_replique`]: https://docs.rs/bevy_replique
//!
//! # The format
//!
//! ```text
//! := start
//! Alice: Hi! How are you?
//! -> Good, thanks!
//!     Alice: Glad to hear it.
//! -> Not great.
//!     Alice: Oh no, what happened?
//! => END
//! ---
//! ```
//!
//! The language is described in [the Replique Book], which is the place to
//! look for how a dialogue is written. This documentation is about reading
//! and running one.
//!
//! [the Replique Book]: https://galdormin.github.io/replique/
//!
//! # Tags
//!
//! The `#name` and `#name:value` tags of a line reach the caller apart from
//! its text:
//!
//! ```rust
//! use replique::RepliqueFile;
//! use replique::dialogue::Tag;
//! use replique::vm::{DialogueEvent, DialogueVm};
//!
//! let file = RepliqueFile::from_source(
//!     ":= start\nAlice: #angry Give it back! #sound:alice_01\n---\n",
//! );
//!
//! let mut vm = DialogueVm::default();
//! let DialogueEvent::Say { text, tags, .. } =
//!     vm.start(file.dialogue.unwrap(), "start").unwrap()
//! else {
//!     panic!("expected a line");
//! };
//!
//! assert_eq!(text, "Give it back!");
//! assert_eq!(tags, [Tag::new("angry"), Tag::with_value("sound", "alice_01")]);
//! ```
//!
//! A choice takes tags the same way, `-> #hostile Attack #cost:3`, and each
//! [`Choice`] of a [`DialogueEvent::Choices`] carries its own.
//!
//! [`Choice`]: vm::Choice
//! [`DialogueEvent::Choices`]: vm::DialogueEvent::Choices
//!
//! # Schema
//!
//! With a [`RepliqueSchema`], the parsed dialogue can be validated against
//! speaker, tags, functions and commands.
//!
//! ```rust
//! use replique::parser::parse;
//! use replique::parser::validation::RepliqueSchema;
//!
//! let schema = RepliqueSchema::from_toml(
//!     r#"
//!     speakers = ["Robin", "Fanny"]
//!
//!     [tags]
//!     sad = { scope = ["Fanny"] }
//!
//!     [commands]
//!     change_mood = "2"
//!     "#,
//! )
//! .unwrap();
//!
//! let mut parsed = parse(
//!     ":= start\nFany: Hi!\nRobin: #sad Hello.\n>> change_mood(Fanny)\n---\n",
//! );
//! parsed.validate(&schema);
//!
//! let codes: Vec<_> = parsed.diagnostics.iter().map(|d| d.kind.code()).collect();
//! assert_eq!(codes, ["unknown-speaker", "tag-out-of-scope", "wrong-arity"]);
//! ```
//!
//! What a schema finds is a warning: the dialogue still compiles and runs.
//! Each section is optional and is not checked when empty. The [`builtins`]
//! are known to every schema. See [`parser::validation`] for the format of the file.
//!
//! [`RepliqueSchema`]: parser::validation::RepliqueSchema
//!
//! # Compiling
//!
//! [`RepliqueFile`] is the front door: it parses and compiles a source in one
//! step, and keeps the [`Diagnostics`] either way. A file with errors yields no
//! [`Dialogue`], so check [`RepliqueFile::has_errors`] before running anything.
//!
//! ```rust
//! use replique::RepliqueFile;
//! use replique::parser::diagnostic::Color;
//!
//! let file = RepliqueFile::from_source(":= start\nAlice: Hi!\n=> nope\n---\n");
//!
//! assert!(file.has_errors());
//! print!("{}", file.render_diagnostics(Color::Never));
//! ```
//!
//! [`Diagnostics`]: parser::diagnostic::Diagnostics
//! [`Dialogue`]: dialogue::Dialogue
//!
//! # Running
//!
//! [`DialogueVm`] is a coroutine: it walks the dialogue until it reaches
//! something only the caller can handle, then suspends and returns a
//! [`DialogueEvent`]. Answering it with [`DialogueVm::resume`] carries on.
//!
//! ```rust
//! use replique::RepliqueFile;
//! use replique::vm::{DialogueEvent, DialogueVm, ResumeEvent};
//!
//! let file = RepliqueFile::from_source(
//!     ":= start\nAlice: Hi!\n-> Hello\n    Alice: Hello!\n-> Bye\n    Alice: Bye!\n---\n",
//! );
//! let dialogue = file.dialogue.unwrap();
//!
//! let mut vm = DialogueVm::default();
//! let mut event = vm.start(dialogue, "start").unwrap();
//!
//! loop {
//!     event = match event {
//!         DialogueEvent::Say { speaker, text, .. } => {
//!             println!("{}: {}", speaker.unwrap_or_default(), text);
//!             vm.resume(ResumeEvent::Advance).unwrap()
//!         }
//!         // A real game would show these and wait for the player.
//!         DialogueEvent::Choices { .. } => vm.resume(ResumeEvent::Select(0)).unwrap(),
//!         DialogueEvent::Finished => break,
//!         _ => vm.resume(ResumeEvent::Advance).unwrap(),
//!     };
//! }
//! ```
//!
//! A [`Choice`] whose `[if]` condition does not hold is still listed, with
//! [`Choice::available`] set to `false`, so that the indexes do not move.
//! Selecting it is a [`VmError::ChoiceNotAvailable`].
//!
//! [`DialogueVm::vars`] reads back every variable a run has written.
//!
//! [`Choice::available`]: vm::Choice::available
//! [`VmError::ChoiceNotAvailable`]: vm::VmError::ChoiceNotAvailable
//! [`DialogueVm::vars`]: vm::DialogueVm::vars
//!
//! A run never panics on a malformed dialogue: every way it can go wrong is a
//! [`VmError`], including the step limit that catches nodes jumping to each
//! other forever.
//!
//! [`DialogueVm`]: vm::DialogueVm
//! [`DialogueVm::resume`]: vm::DialogueVm::resume
//! [`DialogueEvent`]: vm::DialogueEvent
//! [`VmError`]: vm::VmError
//!
//! # Commands and functions
//!
//! A `>> command(...)` *does* something. It surfaces as
//! [`DialogueEvent::Command`], which suspends the VM like a line does, so an
//! animation or a sound can run to completion before the dialogue moves on.
//!
//! A `[function(...)]` *answers* something, and never suspends. The VM resolves
//! it through the [`RepliqueHost`] the caller passes to
//! [`DialogueVm::start_with`] and [`DialogueVm::resume_with`]. The same host
//! answers `has` when it is asked of a subject rather than of a dictionary:
//!
//! ```rust
//! use std::collections::HashMap;
//!
//! use replique::RepliqueFile;
//! use replique::dialogue::Value;
//! use replique::host::{HostError, RepliqueHost};
//! use replique::vm::{DialogueEvent, DialogueVm};
//!
//! struct Game {
//!     gold: i64,
//!     moods: HashMap<&'static str, &'static str>,
//! }
//!
//! impl RepliqueHost for Game {
//!     fn call(&mut self, name: &str, _args: Vec<Value>) -> Result<Value, HostError> {
//!         match name {
//!             "gold" => Ok(Value::Int(self.gold)),
//!             _ => Err(HostError::UnknownFunction(name.into())),
//!         }
//!     }
//!
//!     fn has_feature(&mut self, subject: &str, name: &str) -> Result<bool, HostError> {
//!         if !["Angry", "Happy"].contains(&name) {
//!             return Err(HostError::UnknownFeature(name.into()));
//!         }
//!
//!         match self.moods.get(subject) {
//!             Some(mood) => Ok(*mood == name),
//!             None => Err(HostError::UnknownSubject(subject.into())),
//!         }
//!     }
//! }
//!
//! let file = RepliqueFile::from_source(
//!     r#":= start
//! [if Alice has Angry]
//!     Alice: Give me back my [gold()] coins!
//! [else]
//!     Alice: You have [gold()] coins.
//! ---
//! "#,
//! );
//! let dialogue = file.dialogue.unwrap();
//!
//! let mut game = Game {
//!     gold: 12,
//!     moods: HashMap::from([("Alice", "Angry")]),
//! };
//! let mut vm = DialogueVm::default();
//!
//! let DialogueEvent::Say { text, .. } = vm.start_with(&mut game, dialogue, "start").unwrap()
//! else {
//!     panic!("expected a line");
//! };
//! assert_eq!(text, "Give me back my 12 coins!");
//! ```
//!
//! The [`builtins`] are available to every dialogue, whatever the host.
//!
//! [`DialogueEvent::Command`]: vm::DialogueEvent::Command
//! [`RepliqueHost`]: host::RepliqueHost
//! [`DialogueVm::start_with`]: vm::DialogueVm::start_with
//! [`DialogueVm::resume_with`]: vm::DialogueVm::resume_with
//!
//! # Modules
//!
//! - [`parser`] turns a source into an AST and [`Diagnostics`].
//! - [`dialogue`] is the compiled form the VM walks, and its [`Value`] type.
//! - [`vm`] runs it.
//! - [`host`] is how a dialogue calls into the game.
//! - [`builtins`] are the functions every dialogue can call.
//!
//! [`Value`]: dialogue::Value

use std::path::{Path, PathBuf};

use crate::{
    dialogue::{Dialogue, compiler::compile},
    parser::{
        diagnostic::{Color, Diagnostics},
        parse,
    },
};

pub mod builtins;
pub mod dialogue;
pub mod host;
pub mod parser;
pub mod vm;

#[derive(Debug, Clone)]
pub struct RepliqueFile {
    pub path: Option<PathBuf>,
    pub source: String,
    pub dialogue: Option<Dialogue>,
    pub diagnostics: Diagnostics,
}

impl RepliqueFile {
    pub fn from_source(source: impl Into<String>) -> Self {
        let source = source.into();
        let compiled = compile(parse(&source));
        Self {
            path: None,
            source,
            dialogue: compiled.dialogue,
            diagnostics: compiled.diagnostics,
        }
    }

    pub fn with_path(mut self, path: &Path) -> Self {
        self.path = Some(path.into());
        self.diagnostics = self.diagnostics.with_path(path);
        self
    }

    /// Diagnostics rendered for a terminal
    pub fn render_diagnostics(&self, color: Color) -> String {
        self.diagnostics.render(&self.source, color)
    }

    /// Diagnostics rendered as one line each, for an editor or a CI log.
    pub fn render_diagnostics_short(&self, color: Color) -> String {
        self.diagnostics.render_short(&self.source, color)
    }

    pub fn has_errors(&self) -> bool {
        self.diagnostics.errors() > 0
    }
}
