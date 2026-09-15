//! Running the `>>` commands of a dialogue.
//!
//! The arguments arrive already typed, in the order the dialogue writes them:
//!
//! ```
//! # use bevy::prelude::*;
//! # use bevy::platform::collections::HashSet;
//! # use bevy_replique::prelude::*;
//!
//! #[derive(Resource, Default)]
//! struct Flags(HashSet<String>);
//!
//! /// `>> set_flag("met_alice", true)`
//! fn set_flag(In((flag, on)): In<(String, bool)>, mut flags: ResMut<Flags>) {
//!     if on {
//!         flags.0.insert(flag);
//!     } else {
//!         flags.0.remove(&flag);
//!     }
//! }
//!
//! # let mut app = App::new();
//! # app.init_resource::<Flags>();
//! # app.add_dialogue_command_named("set_flag", set_flag);
//! ```
//!
//! **Declare the name over the system.** `#[replique_command]` writes down the
//! word the dialogue writes after the `>>`, next to what it does, and
//! [`add_dialogue_command`](DialogueCommandAppExt::add_dialogue_command)
//! registers it from there. This is the way to register a command: the name is
//! written once, where it can be read and documented.
//!
//! ```
//! # use bevy::prelude::*;
//! # use bevy::platform::collections::HashSet;
//! # use bevy_replique::prelude::*;
//! # #[derive(Resource, Default)]
//! # struct Flags(HashSet<String>);
//! /// Turns a flag of the save on or off.
//! #[replique_command(name = "set_flag")]
//! fn raise(In((flag, on)): In<(String, bool)>, mut flags: ResMut<Flags>) {
//!     if on {
//!         flags.0.insert(flag);
//!     } else {
//!         flags.0.remove(&flag);
//!     }
//! }
//!
//! # let mut app = App::new();
//! # app.init_resource::<Flags>();
//! app.add_dialogue_command(raise);
//! # assert_eq!(raise::NAME, "set_flag");
//! ```
//!
//! [`add_dialogue_command_named`](DialogueCommandAppExt::add_dialogue_command_named)
//! takes the name as a string instead, for a system the attribute cannot be
//! put on.

use std::fmt::Display;

use bevy::{
    ecs::message::{MessageCursor, Messages},
    platform::collections::HashMap,
    prelude::*,
};

use crate::{
    call::{
        CurrentDialogueCall,
        args::{DialogueArgs, FromDialogueArgs},
    },
    message::{DialogueCommand, ResumeDialogue, ResumeInput},
};

/// What a command answers about the dialogue that called it.
///
/// A command that returns `()` is [`Immediate`]: the runner resumes the
/// dialogue itself, as soon as the command has run. Returning a `CommandFlow`
/// is how a command says it may need longer than that.
///
/// [`Blocking`] only takes effect on a `>> await command()`. The dialogue
/// then stays suspended until the game sends a [`ResumeDialogue`] carrying the
/// [`DialogueToken`] of the call, which [`DialogueCall::token`] hands over.
/// Without the `await`, the call site has not asked to wait: the runner
/// resumes as it would for [`Immediate`], and logs the disagreement.
///
/// [`Immediate`]: Self::Immediate
/// [`Blocking`]: Self::Blocking
/// [`DialogueToken`]: crate::call::DialogueToken
/// [`DialogueCall::token`]: crate::call::DialogueCall::token
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandFlow {
    /// The runner resumes the dialogue on its own.
    Immediate,
    /// The game resumes the dialogue, once whatever the command started is
    /// over. Only honoured on an awaited call.
    Blocking,
}

pub trait IntoCommandFlow {
    fn into_flow(self) -> CommandFlow;
}

impl IntoCommandFlow for () {
    fn into_flow(self) -> CommandFlow {
        CommandFlow::Immediate
    }
}

impl IntoCommandFlow for CommandFlow {
    fn into_flow(self) -> CommandFlow {
        self
    }
}

impl<V, E> IntoCommandFlow for Result<V, E>
where
    V: IntoCommandFlow,
    E: Display,
{
    fn into_flow(self) -> CommandFlow {
        match self {
            Ok(val) => val.into_flow(),
            Err(err) => {
                error!("{err}");
                CommandFlow::Immediate
            }
        }
    }
}

/// Converts the call, runs the system registered for it, and reports what it
/// answered about the dialogue.
type CommandRunner = Box<dyn Fn(&mut World, DialogueArgs) -> CommandFlow + Send + Sync>;

#[derive(Resource, Default)]
pub(crate) struct DialogueCommandRegistry {
    commands: HashMap<String, CommandRunner>,
}

/// A command declared with `#[replique_command]`, name and doc included.
///
/// ```
/// # use bevy::prelude::*;
/// # use bevy_replique::prelude::*;
/// /// Turns a flag of the save on or off.
/// #[replique_command(name = "set_flag")]
/// fn raise(In((name, on)): In<(String, bool)>) {
///     let _ = (name, on);
/// }
///
/// assert_eq!(raise::NAME, "set_flag");
/// assert_eq!(raise::DOC, "Turns a flag of the save on or off.");
/// ```
///
/// Reach it through [`add_dialogue_command`], which is the whole of what it is
/// for.
///
/// [`add_dialogue_command`]: DialogueCommandAppExt::add_dialogue_command
pub trait RepliqueCommand {
    /// What the dialogue writes after the `>>`.
    const NAME: &'static str;
    /// The documentation the attribute read off the system.
    const DOC: &'static str;

    fn register(self, app: &mut App);
}

/// Registers the system to run for a `>>` command.
///
/// Two ways in, and [`add_dialogue_command`] is the one to reach for: the word
/// the dialogue writes then lives with the system it names, where it can be
/// read and documented, instead of being repeated as a string at the point of
/// registration. [`add_dialogue_command_named`] is for a system the attribute
/// cannot be put on.
///
/// Either way, the shape of the input says what the call must look like:
/// `In<(String, f32)>`, `In<String>`, `In<()>`, or
/// [`In<DialogueArgs>`](DialogueArgs) to take the arguments raw. A call that
/// does not fit is logged and skipped, and the dialogue carries on.
///
/// Unlike a function, a command may write to the world, and may leave the
/// dialogue waiting: it is what the `>>` of a line is for.
///
/// [`add_dialogue_command`]: DialogueCommandAppExt::add_dialogue_command
/// [`add_dialogue_command_named`]: DialogueCommandAppExt::add_dialogue_command_named
pub trait DialogueCommandAppExt {
    /// Registers a system declared with `#[replique_command]`, under the name
    /// the attribute gave it.
    ///
    /// This is the way to register a command. There is no name to repeat here,
    /// hence none to get wrong: the word the dialogue writes is the one
    /// written over the system.
    ///
    /// ```
    /// # use bevy::prelude::*;
    /// # use bevy::platform::collections::HashSet;
    /// # use bevy_replique::prelude::*;
    /// # #[derive(Resource, Default)]
    /// # struct Flags(HashSet<String>);
    /// /// `>> set_flag("met_alice", true)`
    /// #[replique_command]
    /// fn set_flag(In((flag, on)): In<(String, bool)>, mut flags: ResMut<Flags>) {
    ///     if on {
    ///         flags.0.insert(flag);
    ///     } else {
    ///         flags.0.remove(&flag);
    ///     }
    /// }
    /// # let mut app = App::new();
    /// # app.init_resource::<Flags>();
    /// app.add_dialogue_command(set_flag);
    /// ```
    ///
    /// Registering the same name twice keeps the last system.
    fn add_dialogue_command<S>(&mut self, system: S) -> &mut Self
    where
        S: RepliqueCommand;

    /// Registers a system under a name given here, for the cases
    /// [`add_dialogue_command`] cannot cover: a system from a crate you do not
    /// control, or a name only known once the game runs.
    ///
    /// Prefer the attribute when you can. A name written here is a second
    /// place to keep in step with the dialogue, and nothing checks that the
    /// two agree.
    ///
    /// ```
    /// # use bevy::prelude::*;
    /// # use bevy::platform::collections::HashSet;
    /// # use bevy_replique::prelude::*;
    /// # #[derive(Resource, Default)]
    /// # struct Flags(HashSet<String>);
    /// /// `>> set_flag("met_alice", true)`
    /// fn set_flag(In((flag, on)): In<(String, bool)>, mut flags: ResMut<Flags>) {
    ///     if on {
    ///         flags.0.insert(flag);
    ///     } else {
    ///         flags.0.remove(&flag);
    ///     }
    /// }
    /// # let mut app = App::new();
    /// # app.init_resource::<Flags>();
    /// app.add_dialogue_command_named("set_flag", set_flag);
    /// ```
    ///
    /// Registering the same name twice keeps the last system.
    ///
    /// [`add_dialogue_command`]: DialogueCommandAppExt::add_dialogue_command
    fn add_dialogue_command_named<T, O, M>(
        &mut self,
        name: impl Into<String>,
        system: impl IntoSystem<In<T>, O, M> + 'static,
    ) -> &mut Self
    where
        T: FromDialogueArgs + Send + Sync + 'static,
        O: IntoCommandFlow + 'static;
}

impl DialogueCommandAppExt for App {
    fn add_dialogue_command_named<T, O, M>(
        &mut self,
        name: impl Into<String>,
        system: impl IntoSystem<In<T>, O, M> + 'static,
    ) -> &mut Self
    where
        T: FromDialogueArgs + Send + Sync + 'static,
        O: IntoCommandFlow + 'static,
    {
        let name = name.into();
        let world = self.world_mut();
        let id = world.register_system(system);

        let label = name.clone();
        let run: CommandRunner = Box::new(move |world, args| {
            // A call that never reached its system cannot be waiting on
            // anything: whatever the signature says, the dialogue carries on.
            let input = match T::from_dialogue_args(args) {
                Ok(input) => input,
                Err(err) => {
                    error!("dialogue command `{label}`: {err}");
                    return CommandFlow::Immediate;
                }
            };

            match world.run_system_with(id, input) {
                Ok(out) => out.into_flow(),
                Err(err) => {
                    error!("dialogue command `{label}` failed: {err}");
                    CommandFlow::Immediate
                }
            }
        });

        world
            .get_resource_or_insert_with(DialogueCommandRegistry::default)
            .commands
            .insert(name, run);

        self
    }

    fn add_dialogue_command<S>(&mut self, system: S) -> &mut Self
    where
        S: RepliqueCommand,
    {
        system.register(self);
        self
    }
}

/// Runs the registered commands, then resumes their runner.
pub(crate) fn run_dialogue_commands(
    world: &mut World,
    mut cursor: Local<MessageCursor<DialogueCommand>>,
) {
    let pending: Vec<DialogueCommand> = {
        let Some(messages) = world.get_resource::<Messages<DialogueCommand>>() else {
            return;
        };
        cursor.read(messages).cloned().collect()
    };

    if pending.is_empty() || !world.contains_resource::<DialogueCommandRegistry>() {
        return;
    }

    // The registry is taken out for the duration, so that the command systems
    // get the `&mut World` they need. They cannot register a command in turn.
    world.resource_scope(|world, registry: Mut<DialogueCommandRegistry>| {
        for command in pending {
            let flow = match registry.commands.get(&command.name) {
                Some(func) => {
                    world.insert_resource(CurrentDialogueCall(command.token));
                    let flow = func(world, DialogueArgs(command.args));
                    world.remove_resource::<CurrentDialogueCall>();
                    flow
                }
                None => {
                    // Command not found raise an error but resume the dialogue
                    error!("dialogue command {}: not found in registry", command.name);
                    CommandFlow::Immediate
                }
            };

            let waits = match (command.awaited, flow) {
                (true, CommandFlow::Blocking) => true,
                (false, CommandFlow::Blocking) => {
                    warn!(
                        "dialogue command `{}` asked to block, but the line does not `await` it: the dialogue carries on",
                        command.name
                    );
                    false
                }
                (_, CommandFlow::Immediate) => false,
            };

            if !waits {
                world.write_message(ResumeDialogue {
                    token: command.token,
                    input: ResumeInput::Advance,
                });
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use bevy::asset::AssetPlugin;
    use replique::dialogue::Value;

    use super::*;
    use crate::{
        call::{DialogueCall, DialogueToken},
        plugin::RepliquePLugin,
    };

    /// What a test command writes down, to prove it ran and with what.
    #[derive(Resource, Default, Debug, PartialEq)]
    struct Ran(Vec<String>);

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default(), RepliquePLugin))
            .init_resource::<Ran>();
        app
    }

    /// A runner entity, no dialogue needed: the command runs off the message.
    fn spawn_runner(app: &mut App) -> Entity {
        app.world_mut().spawn_empty().id()
    }

    /// Sends the command and lets the frame run it.
    fn call(app: &mut App, runner: Entity, name: &str, args: Vec<Value>) {
        call_with(app, runner, name, args, false);
    }

    /// The same, for a `>> await name()` line.
    fn call_awaited(app: &mut App, runner: Entity, name: &str, args: Vec<Value>) {
        call_with(app, runner, name, args, true);
    }

    fn call_with(app: &mut App, runner: Entity, name: &str, args: Vec<Value>, awaited: bool) {
        app.world_mut().write_message(DialogueCommand {
            token: DialogueToken::new(0, runner),
            name: name.to_string(),
            args,
            awaited,
        });
        app.update();
    }

    fn ran(app: &App) -> &[String] {
        &app.world().resource::<Ran>().0
    }

    fn resumed(app: &App) -> Vec<Entity> {
        app.world()
            .resource::<Messages<ResumeDialogue>>()
            .iter_current_update_messages()
            .map(|resume| resume.token.runner())
            .collect()
    }

    #[test]
    fn a_command_takes_its_arguments_already_typed() {
        fn play(In((sound, volume, loops)): In<(String, f32, bool)>, mut ran: ResMut<Ran>) {
            ran.0.push(format!("{sound} {volume} {loops}"));
        }

        let mut app = app();
        app.add_dialogue_command_named("play", play);
        let runner = spawn_runner(&mut app);

        call(
            &mut app,
            runner,
            "play",
            vec![
                Value::String("bell".into()),
                Value::Float(0.5),
                Value::Bool(true),
            ],
        );

        assert_eq!(ran(&app), ["bell 0.5 true"]);
    }

    #[test]
    fn a_command_that_does_not_fit_its_signature_is_skipped_but_resumes() {
        fn play(In(_): In<(String, f32)>, mut ran: ResMut<Ran>) {
            ran.0.push("ran".into());
        }

        let mut app = app();
        app.add_dialogue_command_named("play", play);
        let runner = spawn_runner(&mut app);

        call(&mut app, runner, "play", vec![Value::Bool(true)]);

        assert_eq!(ran(&app), [] as [String; 0]);
        // The dialogue carries on rather than freezing on a typo.
        assert_eq!(resumed(&app), [runner]);
    }

    #[test]
    fn raw_arguments_stay_reachable() {
        fn any(In(args): In<DialogueArgs>, mut ran: ResMut<Ran>) {
            ran.0.push(format!("{}", args.0.len()));
        }

        let mut app = app();
        app.add_dialogue_command_named("any", any);
        let runner = spawn_runner(&mut app);

        call(&mut app, runner, "any", vec![Value::Bool(true)]);

        assert_eq!(ran(&app), ["1".to_string()]);
    }

    #[test]
    fn a_registered_command_resumes_its_runner() {
        fn noop(In(()): In<()>) {}

        let mut app = app();
        app.add_dialogue_command_named("noop", noop);
        let runner = spawn_runner(&mut app);

        call(&mut app, runner, "noop", vec![]);

        assert_eq!(resumed(&app), [runner]);
    }

    #[test]
    fn an_unregistered_command_resumes_tis_runner() {
        let mut app = app();
        let runner = spawn_runner(&mut app);

        call(&mut app, runner, "unknown", vec![]);

        assert_eq!(ran(&app), [] as [String; 0]);
        assert_eq!(resumed(&app), [runner]);
    }

    #[test]
    fn a_command_sees_the_whole_world() {
        #[derive(Component)]
        struct Spawned;

        fn spawn(In(()): In<()>, mut commands: Commands) {
            commands.spawn(Spawned);
        }

        let mut app = app();
        app.add_dialogue_command_named("spawn", spawn);
        let runner = spawn_runner(&mut app);

        call(&mut app, runner, "spawn", vec![]);

        assert_eq!(
            app.world_mut()
                .query::<&Spawned>()
                .iter(app.world())
                .count(),
            1
        );
    }

    /// The two halves of the decision: the line asks to wait, the command
    /// answers that it has something to wait for. Only both together suspend.
    #[test]
    fn an_awaited_blocking_command_leaves_its_runner_suspended() {
        fn play_anim(In(()): In<()>, mut ran: ResMut<Ran>) -> CommandFlow {
            ran.0.push("started".into());
            CommandFlow::Blocking
        }

        let mut app = app();
        app.add_dialogue_command_named("play_anim", play_anim);
        let runner = spawn_runner(&mut app);

        call_awaited(&mut app, runner, "play_anim", vec![]);

        // It ran, and the dialogue is now the game's to resume.
        assert_eq!(ran(&app), ["started"]);
        assert_eq!(resumed(&app), [] as [Entity; 0]);
    }

    /// A command that can wait, on a line that did not ask it to: the call
    /// site wins, and the dialogue carries on.
    #[test]
    fn a_blocking_command_that_is_not_awaited_resumes_anyway() {
        fn play_anim(In(()): In<()>) -> CommandFlow {
            CommandFlow::Blocking
        }

        let mut app = app();
        app.add_dialogue_command_named("play_anim", play_anim);
        let runner = spawn_runner(&mut app);

        call(&mut app, runner, "play_anim", vec![]);

        assert_eq!(resumed(&app), [runner]);
    }

    /// `await` on a command with nothing to wait for is not an error: the
    /// command says it is done, and is taken at its word.
    #[test]
    fn an_awaited_immediate_command_resumes_its_runner() {
        fn set_flag(In(()): In<()>) -> CommandFlow {
            CommandFlow::Immediate
        }

        let mut app = app();
        app.add_dialogue_command_named("set_flag", set_flag);
        let runner = spawn_runner(&mut app);

        call_awaited(&mut app, runner, "set_flag", vec![]);

        assert_eq!(resumed(&app), [runner]);
    }

    /// A command that returns `()` keeps working, and never suspends.
    #[test]
    fn a_command_without_a_flow_is_immediate_even_when_awaited() {
        fn noop(In(()): In<()>) {}

        let mut app = app();
        app.add_dialogue_command_named("noop", noop);
        let runner = spawn_runner(&mut app);

        call_awaited(&mut app, runner, "noop", vec![]);

        assert_eq!(resumed(&app), [runner]);
    }

    /// A blocking command that fails has started nothing, so nothing will
    /// resume the dialogue later: it has to resume now.
    #[test]
    fn an_awaited_command_that_fails_resumes_rather_than_freezing() {
        fn play_anim(In(()): In<()>) -> Result<CommandFlow, String> {
            Err("no such animation".into())
        }

        let mut app = app();
        app.add_dialogue_command_named("play_anim", play_anim);
        let runner = spawn_runner(&mut app);

        call_awaited(&mut app, runner, "play_anim", vec![]);

        assert_eq!(resumed(&app), [runner]);
    }

    /// Arguments that do not fit never reach the system, so whatever its
    /// signature promises, the dialogue cannot be left waiting on it.
    #[test]
    fn an_awaited_command_with_bad_arguments_resumes() {
        fn play_anim(In(_): In<(String, f32)>) -> CommandFlow {
            CommandFlow::Blocking
        }

        let mut app = app();
        app.add_dialogue_command_named("play_anim", play_anim);
        let runner = spawn_runner(&mut app);

        call_awaited(&mut app, runner, "play_anim", vec![Value::Bool(true)]);

        assert_eq!(resumed(&app), [runner]);
    }

    /// The call context is posed for the command and taken back after it.
    #[test]
    fn a_command_reads_the_runner_it_was_called_from() {
        fn who(In(()): In<()>, call: DialogueCall, mut ran: ResMut<Ran>) {
            ran.0.push(format!("{:?}", call.runner()));
        }

        let mut app = app();
        app.add_dialogue_command_named("who", who);
        let runner = spawn_runner(&mut app);

        call(&mut app, runner, "who", vec![]);

        assert_eq!(ran(&app), [format!("{runner:?}")]);
        assert!(!app.world().contains_resource::<CurrentDialogueCall>());
    }

    #[test]
    fn registering_a_name_twice_keeps_the_last_system() {
        fn first(In(()): In<()>, mut ran: ResMut<Ran>) {
            ran.0.push("first".into());
        }
        fn second(In(()): In<()>, mut ran: ResMut<Ran>) {
            ran.0.push("second".into());
        }

        let mut app = app();
        app.add_dialogue_command_named("play", first);
        app.add_dialogue_command_named("play", second);
        let runner = spawn_runner(&mut app);

        call(&mut app, runner, "play", vec![]);

        assert_eq!(ran(&app), ["second"]);
        assert_eq!(
            app.world()
                .resource::<DialogueCommandRegistry>()
                .commands
                .len(),
            1
        );
    }
}
