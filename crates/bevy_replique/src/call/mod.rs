//! What a dialogue calls into the game, and what it learns about the call.
//!
//! Two kinds, told apart by what they are allowed to do: a [`command`] *does*
//! something and may leave the dialogue waiting, a [`function`] *answers*
//! something and never can. [`args`] is the translation both share, from the
//! values a dialogue writes to the types a system asks for.
//!
//! [`DialogueCall`] is the third piece, shared by both: what the arguments do
//! not say, namely which dialogue is calling and how to resume it.

use bevy::{
    ecs::{
        entity::Entity,
        resource::Resource,
        system::{Query, Res, SystemParam},
    },
    prelude::Deref,
};

use crate::runner::DialogueRunner;

pub mod args;
pub mod command;
pub mod function;

/// The right to resume one suspension of one dialogue.
///
/// Every message a runner writes carries the token of the suspension it opens,
/// and a [`ResumeDialogue`] has to hand that same token back. A runner opens a
/// new one each time it moves, so a token answers the suspension it was minted
/// for and no other: an answer that arrives late — a second press on a line,
/// an animation ending after the player skipped it — is dropped rather than
/// advancing whatever the dialogue has reached in the meantime.
///
/// A command that means to wait gets its token from [`DialogueCall::token`],
/// and hands it to whatever will end the wait.
///
/// [`ResumeDialogue`]: crate::message::ResumeDialogue
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DialogueToken {
    runner: Entity,
    seq: u64,
}

impl DialogueToken {
    pub(crate) fn new(seq: u64, runner: Entity) -> Self {
        Self { runner, seq }
    }

    pub fn runner(&self) -> Entity {
        self.runner
    }
}

/// The call in progress, posed by `run_dialogue_commands` for its duration.
#[derive(Resource, Clone, Copy, Deref)]
pub(crate) struct CurrentDialogueCall(DialogueToken);

/// The dialogue that reached this command.
///
/// A command asks for it like any other system parameter, next to its
/// `Commands` and its resources. What the dialogue wrote arrives through
/// `In<T>`; who wrote it arrives here.
///
/// ```
/// # use bevy::prelude::*;
/// # use bevy_replique::prelude::*;
/// # #[derive(Component)]
/// # struct Anim(String);
/// # #[derive(Component)]
/// # struct ResumeOn(DialogueToken);
/// /// `>> await play_anim("wave")`
/// #[replique_command]
/// fn play_anim(In(name): In<String>, call: DialogueCall, mut commands: Commands) -> CommandFlow {
///     commands.spawn((Anim(name), ResumeOn(call.token())));
///     CommandFlow::Blocking
/// }
/// # let mut app = App::new();
/// # app.add_dialogue_command(play_anim);
/// ```
///
/// A function reads it too, and sees the runner that is asking. Its
/// [`token`](Self::token) is the one of the suspension that runner is heading into.
#[derive(SystemParam)]
pub struct DialogueCall<'w, 's> {
    current: Res<'w, CurrentDialogueCall>,
    runners: Query<'w, 's, &'static DialogueRunner>,
}

impl DialogueCall<'_, '_> {
    /// The entity holding the runner that wrote this `>>`.
    pub fn runner(&self) -> Entity {
        self.current.runner()
    }

    /// Its ticket, to hand to whatever will end the wait.
    pub fn token(&self) -> DialogueToken {
        self.current.0
    }

    /// The ticket of *another* runner, for a command that resumes a
    /// neighbouring dialogue. `None` if that entity holds no runner.
    pub fn token_of(&self, runner: Entity) -> Option<DialogueToken> {
        self.runners
            .get(runner)
            .ok()
            .map(|r| r.current_token(runner))
    }
}
