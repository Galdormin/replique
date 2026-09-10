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

use std::any::type_name;

use bevy::{
    ecs::message::{MessageCursor, Messages},
    platform::collections::HashMap,
    prelude::*,
};
use replique::dialogue::Value;
use thiserror::Error;

use crate::message::{DialogueCommand, ResumeDialogue, ResumeInput};

/// A call that does not fit the signature the system asked for.
#[derive(Error, Debug, PartialEq)]
pub enum CommandArgsError {
    #[error("expected at most {expected} argument(s), got {got}")]
    TooManyArgs { expected: usize, got: usize },
    #[error("argument {index} is missing or is not a `{expected}`")]
    Argument {
        index: usize,
        expected: &'static str,
    },
}

/// A call as the dialogue wrote it, arguments left as [`Value`].
///
/// Ask for it as the input of a command in the two cases a typed signature
/// like `In<(String, f32)>` cannot cover.
///
/// **A command whose arity is not fixed.** A tuple pins the number of
/// arguments down; `CommandArgs` takes as many as the writer put:
///
/// ```
/// # use bevy::prelude::*;
/// # use bevy_replique::prelude::*;
///
/// #[derive(Resource, Default)]
/// struct Score(f32);
///
/// /// `>> add(1, 2.0, 3.5, 4)` and `>> add(4, 5)` both fit.
/// fn add(In(args): In<CommandArgs>, mut score: ResMut<Score>) {
///     for value in &args.args {
///         let points = match value {
///             Value::Int(points) => *points as f32,
///             Value::Float(points) => *points as f32,
///             _ => continue,
///         };
///         score.0 += points;
///     }
/// }
/// # let mut app = App::new();
/// # app.init_resource::<Score>();
/// # app.add_dialogue_command("add", add);
/// ```
///
/// **A command that needs the dialogue it came from.** [`runner`] is the
/// entity holding the [`DialogueRunner`], which matters as soon as two
/// dialogues run at the same time — an ambient conversation and the one the
/// player is in:
///
/// ```
/// # use bevy::prelude::*;
/// # use bevy_replique::command::{CommandArgs, DialogueCommandAppExt};
///
/// /// Marks the dialogue that asked not to be interrupted.
/// #[derive(Component)]
/// struct Locked;
///
/// /// `>> lock` locks the dialogue that ran it, not the other one.
/// fn lock(In(args): In<CommandArgs>, mut commands: Commands) {
///     commands.entity(args.runner).insert(Locked);
/// }
/// # let mut app = App::new();
/// # app.add_dialogue_command("lock", lock);
/// ```
///
/// [`FromValue::from_value`] reads a single [`Value`] into a Rust type, for a
/// command that would rather not match on the enum by hand.
///
/// [`runner`]: CommandArgs::runner
/// [`DialogueRunner`]: crate::runner::DialogueRunner
#[derive(Debug, Clone, PartialEq)]
pub struct CommandArgs {
    /// Entity holding the runner that reached the command.
    pub runner: Entity,
    /// Arguments, in the order they are written in the dialogue.
    pub args: Vec<Value>,
}

impl CommandArgs {
    /// Number of arguments the command was called with.
    pub fn len(&self) -> usize {
        self.args.len()
    }

    pub fn is_empty(&self) -> bool {
        self.args.is_empty()
    }
}

/// One argument, read into the type a command system asked for.
///
/// An argument the dialogue did not write never reaches it: that case belongs
/// to [`FromMaybeValue`], which is what a trailing [`Option`] hooks into.
///
/// Implementing it on a type of your own is the usual way to teach the
/// dialogue a new vocabulary: the type then composes into a tuple like any
/// other, and a word the enum does not know is reported as a bad argument
/// instead of reaching the game.
///
/// ```
/// # use bevy::prelude::*;
/// # use bevy_replique::{prelude::*, command::{DialogueCommandAppExt, FromValue}};
///
/// #[derive(Debug, PartialEq)]
/// enum Direction {
///     Left,
///     Right,
/// }
///
/// impl FromValue for Direction {
///     fn from_value(value: Value) -> Option<Self> {
///         // Bare words and quoted text are both strings, so `left` and
///         // `"left"` are the same argument.
///         match String::from_value(value)?.as_str() {
///             "left" => Some(Self::Left),
///             "right" => Some(Self::Right),
///             // `>> face(up)` is refused, and says so in the log.
///             _ => None,
///         }
///     }
/// }
///
/// /// `>> face(left)` and `>> face("right", 0.5)` both fit.
/// fn face(In((direction, speed)): In<(Direction, Option<f32>)>) {
///     let _ = (direction, speed.unwrap_or(1.0));
/// }
/// # let mut app = App::new();
/// # app.add_dialogue_command("face", face);
/// #
/// # assert_eq!(
/// #     Direction::from_value(Value::String("left".into())),
/// #     Some(Direction::Left),
/// # );
/// # assert_eq!(Direction::from_value(Value::String("up".into())), None);
/// ```
pub trait FromValue: Sized {
    fn from_value(value: Value) -> Option<Self>;
}

impl FromValue for Value {
    fn from_value(value: Value) -> Option<Self> {
        Some(value)
    }
}

impl FromValue for String {
    fn from_value(value: Value) -> Option<Self> {
        match value {
            Value::String(s) => Some(s),
            _ => None,
        }
    }
}

impl FromValue for bool {
    fn from_value(value: Value) -> Option<Self> {
        match value {
            Value::Bool(b) => Some(b),
            _ => None,
        }
    }
}

/// One argument, present or not.
///
/// Nothing to implement: every [`FromValue`] gets it, and refuses an argument
/// that is not there. [`Option`] is what accepts the absence, and that is the
/// whole of what a trailing `Option` in a signature means.
pub trait FromMaybeValue: Sized {
    fn from_maybe_value(value: Option<Value>) -> Option<Self>;
}

impl<T: FromValue> FromMaybeValue for T {
    fn from_maybe_value(value: Option<Value>) -> Option<Self> {
        T::from_value(value?)
    }
}

/// An absent argument is the only thing an [`Option`] adds: a present one that
/// does not fit is still an error.
impl<T: FromValue> FromMaybeValue for Option<T> {
    fn from_maybe_value(value: Option<Value>) -> Option<Self> {
        match value {
            None => Some(None),
            Some(value) => T::from_value(value).map(Some),
        }
    }
}

macro_rules! impl_from_value_float {
    ($($T:ty),*) => {$(
        impl FromValue for $T {
            fn from_value(value: Value) -> Option<Self> {
                match value {
                    Value::Float(f) => Some(f as $T),
                    _ => None,
                }
            }
        }
    )*};
}

impl_from_value_float!(f32, f64);

macro_rules! impl_from_value_int {
    ($($T:ty),*) => {$(
        impl FromValue for $T {
            /// A number with a fractional part is not an integer, and is
            /// refused rather than silently truncated.
            fn from_value(value: Value) -> Option<Self> {
                match value {
                    Value::Int(i) => Some(i as $T),
                    _ => None,
                }
            }
        }
    )*};
}

impl_from_value_int!(i8, i16, i32, i64, isize, u8, u16, u32, u64, usize);

/// The whole call, read into the input a command system asked for.
///
/// Implemented for [`CommandArgs`], for a single [`FromValue`], and for tuples
/// of them up to eight. Implement [`FromValue`] rather than this one to give a
/// type of your own to a single argument — it composes into those tuples for
/// free.
///
/// This trait is for a type that reads the *whole* call, when the arguments do
/// not line up one-to-one with the fields: a command that dispatches on its
/// first argument, and whose arity depends on it.
///
/// ```
/// # use bevy::prelude::*;
/// # use bevy_replique::command::{
/// #     CommandArgs, CommandArgsError, DialogueCommandAppExt, FromCommandArgs, FromMaybeValue,
/// # };
/// /// `>> camera(shake)` or `>> camera(move, 120, 40)`
/// #[derive(Debug, PartialEq)]
/// enum Camera {
///     Shake,
///     MoveTo { x: f32, y: f32 },
/// }
///
/// impl FromCommandArgs for Camera {
///     fn from_command_args(args: CommandArgs) -> Result<Self, CommandArgsError> {
///         // The index makes the log point at the argument that is wrong.
///         let bad = |index| CommandArgsError::Argument { index, expected: "Camera" };
///
///         // The arguments are read one by one, with the same
///         // `FromMaybeValue` the tuples use: absent is refused here.
///         let mut values = args.args.into_iter();
///         let order = String::from_maybe_value(values.next()).ok_or(bad(0))?;
///
///         match order.as_str() {
///             "shake" => Ok(Self::Shake),
///             "move" => Ok(Self::MoveTo {
///                 x: f32::from_maybe_value(values.next()).ok_or(bad(1))?,
///                 y: f32::from_maybe_value(values.next()).ok_or(bad(2))?,
///             }),
///             _ => Err(bad(0)),
///         }
///     }
/// }
///
/// fn move_camera(In(order): In<Camera>) {
///     let _ = order;
/// }
/// # let mut app = App::new();
/// # app.add_dialogue_command("camera", move_camera);
/// ```
///
/// A call that does not fit is logged and skipped, and the dialogue carries on:
/// the error you return is what the log shows.
pub trait FromCommandArgs: Sized {
    fn from_command_args(args: CommandArgs) -> Result<Self, CommandArgsError>;
}

impl FromCommandArgs for CommandArgs {
    fn from_command_args(args: CommandArgs) -> Result<Self, CommandArgsError> {
        Ok(args)
    }
}

impl FromCommandArgs for () {
    fn from_command_args(args: CommandArgs) -> Result<Self, CommandArgsError> {
        match args.len() {
            0 => Ok(()),
            got => Err(CommandArgsError::TooManyArgs { expected: 0, got }),
        }
    }
}

/// `alloc::string::String` reads as `String` in an error message.
fn short_type_name<T>() -> &'static str {
    let name = type_name::<T>();
    match name.rsplit_once("::") {
        Some((_, short)) => short,
        None => name,
    }
}

macro_rules! impl_from_command_args {
    ($($T:ident),+) => {
        impl<$($T: FromMaybeValue),+> FromCommandArgs for ($($T,)+) {
            #[allow(unused_assignments, non_snake_case)]
            fn from_command_args(args: CommandArgs) -> Result<Self, CommandArgsError> {
                let expected = [$(stringify!($T)),+].len();
                if args.len() > expected {
                    return Err(CommandArgsError::TooManyArgs { expected, got: args.len() });
                }

                let mut values = args.args.into_iter();
                let mut index = 0;
                $(
                    let $T = $T::from_maybe_value(values.next()).ok_or(
                        CommandArgsError::Argument { index, expected: short_type_name::<$T>() },
                    )?;
                    index += 1;
                )+

                Ok(($($T,)+))
            }
        }
    };
}

impl_from_command_args!(A);
impl_from_command_args!(A, B);
impl_from_command_args!(A, B, C);
impl_from_command_args!(A, B, C, D);
impl_from_command_args!(A, B, C, D, E);
impl_from_command_args!(A, B, C, D, E, F);
impl_from_command_args!(A, B, C, D, E, F, G);
impl_from_command_args!(A, B, C, D, E, F, G, H);

macro_rules! impl_from_command_args_single {
    ($($T:ty),*) => {$(
        impl FromCommandArgs for $T {
            fn from_command_args(args: CommandArgs) -> Result<Self, CommandArgsError> {
                <($T,)>::from_command_args(args).map(|(value,)| value)
            }
        }
    )*};
}

impl_from_command_args_single!(
    Value, String, bool, f32, f64, i8, i16, i32, i64, isize, u8, u16, u32, u64, usize
);

/// Converts the call, then runs the system registered for it.
type CommandRunner = Box<dyn Fn(&mut World, CommandArgs) + Send + Sync>;

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
    /// [`In<CommandArgs>`](CommandArgs) to take them raw.
    ///
    /// Registering the same name twice keeps the last system.
    fn add_dialogue_command<T, M>(
        &mut self,
        name: impl Into<String>,
        system: impl IntoSystem<In<T>, (), M> + 'static,
    ) -> &mut Self
    where
        T: FromCommandArgs + Send + Sync + 'static;
}

impl DialogueCommandAppExt for App {
    fn add_dialogue_command<T, M>(
        &mut self,
        name: impl Into<String>,
        system: impl IntoSystem<In<T>, (), M> + 'static,
    ) -> &mut Self
    where
        T: FromCommandArgs + Send + Sync + 'static,
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
                CommandArgs {
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
        fn any(In(args): In<CommandArgs>, mut ran: ResMut<Ran>) {
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
        let args = |args: Vec<Value>| CommandArgs {
            runner: Entity::PLACEHOLDER,
            args,
        };

        assert_eq!(
            <(String, f32)>::from_command_args(args(vec![Value::String("a".into())])),
            Err(CommandArgsError::Argument {
                index: 1,
                expected: "f32"
            })
        );
        assert_eq!(
            <(String,)>::from_command_args(args(vec![
                Value::String("a".into()),
                Value::Bool(true)
            ])),
            Err(CommandArgsError::TooManyArgs {
                expected: 1,
                got: 2
            })
        );
        assert_eq!(
            <()>::from_command_args(args(vec![Value::Bool(true)])),
            Err(CommandArgsError::TooManyArgs {
                expected: 0,
                got: 1
            })
        );
    }
}
