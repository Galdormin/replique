//! [Replique] dialogues, as a Bevy plugin.
//!
//! [`replique`] parses and runs the dialogue; this crate wires it into Bevy.
//! A `.rep` file is an asset, a running dialogue is an entity, and the game
//! talks to it with messages.
//!
//! [Replique]: https://github.com/Galdormin/replique
//!
//! # A dialogue
//!
//! Dialogues are plain text. Lines, choices and jumps make up most of a file;
//! `[let]` holds state, `[if]` and `[while]` shape what gets played, and
//! anything between `[` and `]` inside a line is evaluated and inserted where
//! it stands. `assets/dialogue/shop.rep`:
//!
//! ```text
//! := shop
//! Merchant: Welcome! You have [gold()] coins.
//!
//! [if gold() >= 10]
//!     Merchant: The lantern is yours.
//!     >> buy("lantern")
//! [else]
//!     Merchant: Come back when you can pay.
//!
//! [let $bells = 3]
//! [while $bells > 0]
//!     [let $bells = $bells - 1]
//!     >> await ring_bell()
//!
//! -> Thanks!
//!     Merchant: Any time.
//! -> Say nothing
//!     Merchant: ...
//! ---
//! ```
//!
//! `gold()` and `buy(...)` are the game's own, registered as Bevy systems — see
//! [Commands and functions](#commands-and-functions) below. The full format is
//! documented in [`replique`].
//!
//! # Setup
//!
//! Add [`RepliquePlugin`], load a dialogue, spawn a [`DialogueRunner`] for it
//! and send it a [`StartDialogue`]. The asset does not have to be loaded yet:
//! the runner holds the start pending until it is.
//!
//! ```rust,no_run
//! use bevy::prelude::*;
//! use bevy_replique::prelude::*;
//!
//! fn main() {
//!     App::new()
//!         .add_plugins(DefaultPlugins)
//!         .add_plugins(RepliquePlugin)
//!         .add_systems(Startup, setup)
//!         .add_systems(Update, show_line)
//!         .run();
//! }
//!
//! fn setup(
//!     mut commands: Commands,
//!     assets: Res<AssetServer>,
//!     mut start: MessageWriter<StartDialogue>,
//! ) {
//!     let dialogue: Handle<RepliqueDialogue> = assets.load("dialogue/shop.rep");
//!     let runner = commands.spawn(DialogueRunner::new(dialogue)).id();
//!
//!     start.write(StartDialogue {
//!         runner,
//!         node: "start".into(),
//!     });
//! }
//!
//! /// Show every line, then ask the runner to carry on.
//! fn show_line(
//!     mut lines: MessageReader<DialogueLine>,
//!     mut resume: MessageWriter<ResumeDialogue>,
//! ) {
//!     for line in lines.read() {
//!         match &line.speaker {
//!             Some(speaker) => println!("{speaker}: {}", line.text),
//!             None => println!("{}", line.text),
//!         }
//!
//!         resume.write(ResumeDialogue {
//!             token: line.token,
//!             input: ResumeInput::Advance,
//!         });
//!     }
//! }
//! ```
//!
//! # Driving a dialogue
//!
//! A runner suspends every time it needs the game, and reports why:
//! [`DialogueLine`] for a line, [`DialogueChoices`] for a choice point,
//! [`DialogueCommand`] for a `>>` command, [`DialogueFinished`] at the end.
//!
//! Each of them carries a [`DialogueToken`], which a [`ResumeDialogue`] has to
//! hand back to move the dialogue on — [`ResumeInput::Advance`] after a line or
//! a command, [`ResumeInput::Select`] after choices. A runner mints a new token
//! every time it moves, so an answer that arrives late (a second press on a
//! line, an animation ending after the player skipped it) is dropped instead
//! of advancing whatever the dialogue has reached in the meantime.
//!
//! Because every message names its runner, several dialogues can run at once.
//!
//! # Commands and functions
//!
//! A dialogue reaches into the game in two ways. A `>> command(...)` *does*
//! something and may leave the dialogue waiting; a `[function(...)]` *answers*
//! something and never can. Both are plain Bevy systems, taking their arguments
//! already typed through `In`:
//!
//! ```rust
//! # use bevy::prelude::*;
//! # use bevy::platform::collections::HashSet;
//! use bevy_replique::prelude::*;
//!
//! #[derive(Resource, Default)]
//! struct Flags(HashSet<String>);
//!
//! /// `>> set_flag("met_alice", true)`
//! #[replique_command]
//! fn set_flag(In((flag, on)): In<(String, bool)>, mut flags: ResMut<Flags>) {
//!     if on {
//!         flags.0.insert(flag);
//!     } else {
//!         flags.0.remove(&flag);
//!     }
//! }
//!
//! /// `Alice: You have [gold()] coins.`
//! #[replique_function]
//! fn gold(In(()): In<()>, flags: Res<Flags>) -> i64 {
//!     flags.0.len() as i64
//! }
//!
//! # let mut app = App::new();
//! # app.init_resource::<Flags>();
//! app.add_dialogue_command(set_flag)
//!     .add_dialogue_function(gold);
//! ```
//!
//! A command that means to make the dialogue wait takes the token off
//! [`DialogueCall`] and hands it to whatever will end the wait.
//! See [`call::command`] for that, and [`call::function`] for what a
//! function may return.
//!
//! # Modules
//!
//! - [`asset`] loads `.rep` files, and reports parse errors as asset errors.
//! - [`plugin`] is the plugin and its system sets.
//! - [`runner`] is the [`DialogueRunner`] component driving one dialogue.
//! - [`message`] is everything the game and a runner send each other.
//! - [`call`] is commands, functions, and the conversion of their arguments.
//!
//! # Examples
//!
//! The [`examples`] directory has three runnable ones, from a minimal UI to
//! custom commands and functions:
//!
//! ```sh
//! cargo run -p bevy_replique --example bevy_simple
//! ```
//!
//! [`examples`]: https://github.com/Galdormin/replique/tree/main/crates/bevy_replique/examples
//! [`DialogueRunner`]: runner::DialogueRunner
//! [`StartDialogue`]: message::StartDialogue
//! [`DialogueLine`]: message::DialogueLine
//! [`DialogueChoices`]: message::DialogueChoices
//! [`DialogueCommand`]: message::DialogueCommand
//! [`DialogueFinished`]: message::DialogueFinished
//! [`ResumeDialogue`]: message::ResumeDialogue
//! [`ResumeInput::Advance`]: message::ResumeInput::Advance
//! [`ResumeInput::Select`]: message::ResumeInput::Select
//! [`DialogueToken`]: call::DialogueToken
//! [`DialogueCall`]: call::DialogueCall
//! [`RepliquePlugin`]: plugin::RepliquePlugin

pub mod asset;
pub mod call;
pub mod message;
pub mod plugin;
pub mod runner;

pub mod prelude {
    pub use bevy_replique_derive::{
        RepliqueArgs, RepliqueValue, replique_command, replique_function,
    };
    pub use replique::dialogue::{NodeName, Value, ValueType};

    pub use crate::asset::*;
    pub use crate::call::args::*;
    pub use crate::call::command::*;
    pub use crate::call::function::*;
    pub use crate::call::*;
    pub use crate::message::*;
    pub use crate::plugin::*;
    pub use crate::runner::*;
}
