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

use bevy::{
    ecs::message::{MessageCursor, Messages},
    platform::collections::HashMap,
    prelude::*,
};

use crate::{
    args::{DialogueArgs, FromDialogueArgs},
    message::{DialogueCommand, ResumeDialogue, ResumeInput},
};

/// Converts the call, then runs the system registered for it.
type CommandRunner = Box<dyn Fn(&mut World, DialogueArgs) + Send + Sync>;

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
    fn add_dialogue_command_named<T, M>(
        &mut self,
        name: impl Into<String>,
        system: impl IntoSystem<In<T>, (), M> + 'static,
    ) -> &mut Self
    where
        T: FromDialogueArgs + Send + Sync + 'static;
}

impl DialogueCommandAppExt for App {
    fn add_dialogue_command_named<T, M>(
        &mut self,
        name: impl Into<String>,
        system: impl IntoSystem<In<T>, (), M> + 'static,
    ) -> &mut Self
    where
        T: FromDialogueArgs + Send + Sync + 'static,
    {
        let name = name.into();
        let world = self.world_mut();
        let id = world.register_system(system);

        let label = name.clone();
        let run: CommandRunner = Box::new(move |world, args| {
            let input = match T::from_dialogue_args(args) {
                Ok(input) => input,
                Err(err) => return error!("dialogue command `{label}`: {err}"),
            };

            if let Err(err) = world.run_system_with(id, input) {
                error!("dialogue command `{label}` failed: {err}");
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
pub(super) fn run_dialogue_commands(
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
            if let Some(func) = registry.commands.get(&command.name) {
                func(
                    world,
                    DialogueArgs {
                        runner: command.runner,
                        args: command.args,
                    },
                );
            } else {
                // Command not found raise an error but resume the dialogue
                error!("dialogue command {}: not found in registry", command.name);
            }

            world.write_message(ResumeDialogue {
                runner: command.runner,
                input: ResumeInput::Advance,
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use bevy::asset::AssetPlugin;
    use replique::dialogue::Value;

    use super::*;
    use crate::plugin::RepliquePLugin;

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
        app.world_mut().write_message(DialogueCommand {
            runner,
            name: name.to_string(),
            args,
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
            .map(|resume| resume.runner)
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
    fn raw_arguments_and_the_runner_stay_reachable() {
        fn any(In(args): In<DialogueArgs>, mut ran: ResMut<Ran>) {
            ran.0.push(format!("{:?} {}", args.runner, args.len()));
        }

        let mut app = app();
        app.add_dialogue_command_named("any", any);
        let runner = spawn_runner(&mut app);

        call(&mut app, runner, "any", vec![Value::Bool(true)]);

        assert_eq!(ran(&app), [format!("{runner:?} 1")]);
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
