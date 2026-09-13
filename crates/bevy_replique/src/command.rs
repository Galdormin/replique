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
//! app.add_dialogue_command("set_flag", set_flag);
//! ```

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

/// Registers the system to run for a `>>` command.
pub trait DialogueCommandAppExt {
    /// `name` is what the dialogue writes after the `>>`, and `T` the shape its
    /// arguments must have: `In<(String, f32)>`, `In<String>`, `In<()>`, or
    /// [`In<DialogueArgs>`](DialogueArgs) to take them raw.
    ///
    /// Registering the same name twice keeps the last system.
    fn add_dialogue_command<T, M>(
        &mut self,
        name: impl Into<String>,
        system: impl IntoSystem<In<T>, (), M> + 'static,
    ) -> &mut Self
    where
        T: FromDialogueArgs + Send + Sync + 'static;
}

impl DialogueCommandAppExt for App {
    fn add_dialogue_command<T, M>(
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
        app.add_dialogue_command("play", play);
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
        app.add_dialogue_command("play", play);
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
        app.add_dialogue_command("any", any);
        let runner = spawn_runner(&mut app);

        call(&mut app, runner, "any", vec![Value::Bool(true)]);

        assert_eq!(ran(&app), [format!("{runner:?} 1")]);
    }

    #[test]
    fn a_registered_command_resumes_its_runner() {
        fn noop(In(()): In<()>) {}

        let mut app = app();
        app.add_dialogue_command("noop", noop);
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
        app.add_dialogue_command("spawn", spawn);
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
        app.add_dialogue_command("play", first);
        app.add_dialogue_command("play", second);
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
