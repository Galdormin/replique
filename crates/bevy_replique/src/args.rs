//!

use std::any::type_name;

use bevy::ecs::entity::Entity;
use replique::dialogue::Value;
use thiserror::Error;

/// A call that does not fit the signature the system asked for.
#[derive(Error, Debug, PartialEq)]
pub enum DialogueArgsError {
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
/// arguments down; `DialogueArgs` takes as many as the writer put:
///
/// ```
/// # use bevy::prelude::*;
/// # use bevy_replique::prelude::*;
///
/// #[derive(Resource, Default)]
/// struct Score(f32);
///
/// /// `>> add(1, 2.0, 3.5, 4)` and `>> add(4, 5)` both fit.
/// fn add(In(args): In<DialogueArgs>, mut score: ResMut<Score>) {
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
/// # use bevy_replique::prelude::*;
///
/// /// Marks the dialogue that asked not to be interrupted.
/// #[derive(Component)]
/// struct Locked;
///
/// /// `>> lock` locks the dialogue that ran it, not the other one.
/// fn lock(In(args): In<DialogueArgs>, mut commands: Commands) {
///     commands.entity(args.runner).insert(Locked);
/// }
/// # let mut app = App::new();
/// # app.add_dialogue_command("lock", lock);
/// ```
///
/// [`FromValue::from_value`] reads a single [`Value`] into a Rust type, for a
/// command that would rather not match on the enum by hand.
///
/// [`runner`]: DialogueArgs::runner
/// [`DialogueRunner`]: crate::runner::DialogueRunner
#[derive(Debug, Clone, PartialEq)]
pub struct DialogueArgs {
    /// Entity holding the runner that reached the command.
    pub runner: Entity,
    /// Arguments, in the order they are written in the dialogue.
    pub args: Vec<Value>,
}

impl DialogueArgs {
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
/// # use bevy_replique::prelude::*;
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
/// Implemented for [`DialogueArgs`], for a single [`FromValue`], and for tuples
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
/// # use bevy_replique::prelude::*;
/// /// `>> camera(shake)` or `>> camera(move, 120, 40)`
/// #[derive(Debug, PartialEq)]
/// enum Camera {
///     Shake,
///     MoveTo { x: f32, y: f32 },
/// }
///
/// impl FromDialogueArgs for Camera {
///     fn from_command_args(args: DialogueArgs) -> Result<Self, DialogueArgsError> {
///         // The index makes the log point at the argument that is wrong.
///         let bad = |index| DialogueArgsError::Argument { index, expected: "Camera" };
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
pub trait FromDialogueArgs: Sized {
    fn from_command_args(args: DialogueArgs) -> Result<Self, DialogueArgsError>;
}

impl FromDialogueArgs for DialogueArgs {
    fn from_command_args(args: DialogueArgs) -> Result<Self, DialogueArgsError> {
        Ok(args)
    }
}

impl FromDialogueArgs for () {
    fn from_command_args(args: DialogueArgs) -> Result<Self, DialogueArgsError> {
        match args.len() {
            0 => Ok(()),
            got => Err(DialogueArgsError::TooManyArgs { expected: 0, got }),
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
        impl<$($T: FromMaybeValue),+> FromDialogueArgs for ($($T,)+) {
            #[allow(unused_assignments, non_snake_case)]
            fn from_command_args(args: DialogueArgs) -> Result<Self, DialogueArgsError> {
                let expected = [$(stringify!($T)),+].len();
                if args.len() > expected {
                    return Err(DialogueArgsError::TooManyArgs { expected, got: args.len() });
                }

                let mut values = args.args.into_iter();
                let mut index = 0;
                $(
                    let $T = $T::from_maybe_value(values.next()).ok_or(
                        DialogueArgsError::Argument { index, expected: short_type_name::<$T>() },
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
        impl FromDialogueArgs for $T {
            fn from_command_args(args: DialogueArgs) -> Result<Self, DialogueArgsError> {
                <($T,)>::from_command_args(args).map(|(value,)| value)
            }
        }
    )*};
}

impl_from_command_args_single!(
    Value, String, bool, f32, f64, i8, i16, i32, i64, isize, u8, u16, u32, u64, usize
);
