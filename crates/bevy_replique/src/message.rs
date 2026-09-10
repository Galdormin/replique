//! Messages exchanged between the dialogue runner and the game.
//!
//! Messages come in two directions:
//! - *outbound*: [`DialogueLine`], [`DialogueCommand`], [`DialogueChoices`] and
//!   [`DialogueFinished`] are
//!   sent by a [`DialogueRunner`] to report what the dialogue virtual machine
//!   produced.
//! - *inbound*: [`StartDialogue`] and [`ResumeDialogue`] are sent to a
//!   [`DialogueRunner`] to enter a node, then to drive it forward every time it
//!   is waiting for the player.
//!
//! Every message carries the [`Entity`] holding the [`DialogueRunner`] it belongs
//! to, so several dialogues can run at the same time.
//!
//! [`DialogueRunner`]: crate::runner::DialogueRunner

use bevy::{ecs::entity::Entity, prelude::Message};
use replique::{dialogue::Value, vm::ResumeEvent};

/// A line of dialogue is ready to be displayed.
///
/// Sent by a [`DialogueRunner`] when the virtual machine yields
/// [`DialogueEvent::Say`]. The dialogue stays suspended until a
/// [`ResumeDialogue`] with [`ResumeInput::Advance`] is sent back for the same
/// runner.
///
/// [`DialogueRunner`]: crate::runner::DialogueRunner
/// [`DialogueEvent::Say`]: replique::vm::DialogueEvent::Say
#[derive(Message, Debug, Clone)]
pub struct DialogueLine {
    /// Entity holding the runner that produced this line.
    pub runner: Entity,
    /// Who is speaking, or `None` for a line without a speaker.
    pub speaker: Option<String>,
    /// Text of the line.
    pub text: String,
}

/// The dialogue reached a `>>` command.
///
/// Sent by a [`DialogueRunner`] when the virtual machine yields
/// [`DialogueEvent::Command`]. What the command does is up to the game; the
/// dialogue stays suspended until a [`ResumeDialogue`] with
/// [`ResumeInput::Advance`] is sent back for the same runner, so an animation
/// or a sound can run to completion first.
///
/// [`DialogueRunner`]: crate::runner::DialogueRunner
/// [`DialogueEvent::Command`]: replique::vm::DialogueEvent::Command
#[derive(Message, Debug, Clone)]
pub struct DialogueCommand {
    /// Entity holding the runner that produced this command.
    pub runner: Entity,
    /// Name of the command, as written after the `>>`.
    pub name: String,
    /// Arguments, in the order they are declared.
    pub args: Vec<Value>,
}

/// The dialogue reached a choice point and waits for input.
///
/// Sent by a [`DialogueRunner`] when the virtual machine yields
/// [`DialogueEvent::Choices`]. The dialogue stays suspended until a
/// [`ResumeDialogue`] with [`ResumeInput::Select`] is sent back for the same
/// runner.
///
/// [`DialogueRunner`]: crate::runner::DialogueRunner
/// [`DialogueEvent::Choices`]: replique::vm::DialogueEvent::Choices
#[derive(Message, Debug, Clone)]
pub struct DialogueChoices {
    /// Entity holding the runner that produced these choices.
    pub runner: Entity,
    /// Available choices, in the order they are declared in the dialogue.
    pub choices: Vec<DialogueChoice>,
}

/// A single option of a [`DialogueChoices`].
#[derive(Debug, Clone)]
pub struct DialogueChoice {
    /// Position of the choice in [`DialogueChoices::choices`], to be sent back in
    /// [`ResumeInput::Select`].
    pub index: usize,
    /// Text of the choice.
    pub text: String,
}

/// The dialogue reached its end.
///
/// Sent by a [`DialogueRunner`] when the virtual machine yields
/// [`DialogueEvent::Finished`]. No further message will be sent for this runner
/// until a new [`StartDialogue`] is sent.
///
/// [`DialogueRunner`]: crate::runner::DialogueRunner
/// [`DialogueEvent::Finished`]: replique::vm::DialogueEvent::Finished
#[derive(Message, Debug, Clone)]
pub struct DialogueFinished {
    /// Entity holding the runner that finished.
    pub runner: Entity,
}

/// Ask a [`DialogueRunner`] to enter a dialogue node.
///
/// The runner runs the node until it produces a [`DialogueLine`], a
/// [`DialogueChoices`] or a [`DialogueFinished`]. Only a runner that has not
/// started yet, or whose previous dialogue is finished, can be started; the
/// runner reports an error otherwise.
///
/// [`DialogueRunner`]: crate::runner::DialogueRunner
#[derive(Message, Debug, Clone)]
pub struct StartDialogue {
    /// Entity holding the runner to start.
    pub runner: Entity,
    /// Name of the node to enter.
    pub node: String,
}

/// How a suspended dialogue is resumed.
///
/// The variant must match what the runner is waiting on, otherwise the runner
/// reports an error and the dialogue stays suspended.
#[derive(Debug, Clone)]
pub enum ResumeInput {
    /// Move to the next step, in answer to a [`DialogueLine`] or a
    /// [`DialogueCommand`].
    Advance,
    /// Follow the choice at this [`DialogueChoice::index`], in answer to a
    /// [`DialogueChoices`].
    Select(usize),
}

impl ResumeInput {
    pub fn to_resume_event(&self) -> ResumeEvent {
        match self {
            ResumeInput::Advance => ResumeEvent::Advance,
            ResumeInput::Select(i) => ResumeEvent::Select(*i),
        }
    }
}

/// Ask a [`DialogueRunner`] suspended on a [`DialogueLine`] or a
/// [`DialogueChoices`] to carry on.
///
/// The runner runs until the next [`DialogueLine`], [`DialogueChoices`] or
/// [`DialogueFinished`].
///
/// [`DialogueRunner`]: crate::runner::DialogueRunner
#[derive(Message, Debug, Clone)]
pub struct ResumeDialogue {
    /// Entity holding the runner to resume.
    pub runner: Entity,
    /// Player input driving the dialogue forward.
    pub input: ResumeInput,
}
