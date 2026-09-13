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

/// The systems to call, by command name.
#[derive(Resource, Default)]
pub struct DialogueCommandRegistry {
    commands: HashMap<String, CommandRunner>,
}

impl DialogueCommandRegistry {
    pub fn contains(&self, name: &str) -> bool {
        self.commands.contains_key(name)
    }

    /// The system stays registered in the world, it is only unreachable from
    /// the dialogue.
    pub fn remove(&mut self, name: &str) -> bool {
        self.commands.remove(name).is_some()
    }

    pub fn len(&self) -> usize {
        self.commands.len()
    }

    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }
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
            let input = match T::from_command_args(args) {
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
            let Some(func) = registry.commands.get(&command.name) else {
                continue;
            };

            func(
                world,
                DialogueArgs {
                    runner: command.runner,
                    args: command.args,
                },
            );

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
    use crate::{args::DialogueArgsError, plugin::RepliquePLugin};

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
    fn a_single_argument_needs_no_tuple() {
        fn say(In(text): In<String>, mut ran: ResMut<Ran>) {
            ran.0.push(text);
        }

        let mut app = app();
        app.add_dialogue_command("say", say);
        let runner = spawn_runner(&mut app);

        call(&mut app, runner, "say", vec![Value::String("hi".into())]);

        assert_eq!(ran(&app), ["hi"]);
    }

    #[test]
    fn a_command_can_take_no_argument_at_all() {
        fn fade(In(()): In<()>, mut ran: ResMut<Ran>) {
            ran.0.push("fade".into());
        }

        let mut app = app();
        app.add_dialogue_command("fade", fade);
        let runner = spawn_runner(&mut app);

        call(&mut app, runner, "fade", vec![]);

        assert_eq!(ran(&app), ["fade"]);
    }

    #[test]
    fn a_trailing_option_makes_an_argument_optional() {
        fn play(In((sound, volume)): In<(String, Option<f32>)>, mut ran: ResMut<Ran>) {
            ran.0.push(format!("{sound} {volume:?}"));
        }

        let mut app = app();
        app.add_dialogue_command("play", play);
        let runner = spawn_runner(&mut app);

        call(&mut app, runner, "play", vec![Value::String("a".into())]);
        call(
            &mut app,
            runner,
            "play",
            vec![Value::String("b".into()), Value::Float(1.0)],
        );

        assert_eq!(ran(&app), ["a None", "b Some(1.0)"]);
    }

    #[test]
    fn an_integer_can_be_reads_as_a_float() {
        fn wait(In(duration): In<f32>, mut ran: ResMut<Ran>) {
            ran.0.push(duration.to_string());
        }

        let mut app = app();
        app.add_dialogue_command("wait", wait);
        let runner = spawn_runner(&mut app);

        call(&mut app, runner, "wait", vec![Value::Float(3.0)]);
        call(&mut app, runner, "wait", vec![Value::Int(3)]);

        assert_eq!(ran(&app), ["3"]);
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
    fn an_unregistered_command_is_left_to_the_game() {
        let mut app = app();
        let runner = spawn_runner(&mut app);

        call(&mut app, runner, "unknown", vec![]);

        assert_eq!(ran(&app), [] as [String; 0]);
        // No resume: the dialogue waits for whoever reads the message.
        assert_eq!(resumed(&app), []);
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
        assert_eq!(app.world().resource::<DialogueCommandRegistry>().len(), 1);
    }

    #[test]
    fn a_call_is_checked_against_the_signature_it_targets() {
        let args = |args: Vec<Value>| DialogueArgs {
            runner: Entity::PLACEHOLDER,
            args,
        };

        assert_eq!(
            <(String, f32)>::from_command_args(args(vec![Value::String("a".into())])),
            Err(DialogueArgsError::Argument {
                index: 1,
                expected: "f32"
            })
        );
        assert_eq!(
            <(String,)>::from_command_args(args(vec![
                Value::String("a".into()),
                Value::Bool(true)
            ])),
            Err(DialogueArgsError::TooManyArgs {
                expected: 1,
                got: 2
            })
        );
        assert_eq!(
            <()>::from_command_args(args(vec![Value::Bool(true)])),
            Err(DialogueArgsError::TooManyArgs {
                expected: 0,
                got: 1
            })
        );
    }
}
