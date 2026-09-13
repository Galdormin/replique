//! Reading the arguments of a call into the types a system asks for.
//!
//! A dialogue writes values, not Rust types: `>> play("bell", 0.5)` reaches
//! this crate as a `Vec<Value>`, and the system registered for `play` would
//! rather be handed a `String` and an `f32`. That translation is what this
//! module is, and it is shared by everything a dialogue can call.
//!
//! Three traits, layered:
//!
//! - [`FromValue`] reads *one* value into one type. This is the one to
//!   implement to teach the dialogue a vocabulary of your own, e.g. a
//!   `Direction`, a `Mood`.
//! - [`FromMaybeValue`] is [`FromValue`] plus the absence of an argument.
//!   Nothing to implement: every [`FromValue`] gets it, and refuses an
//!   argument that is not there. A trailing [`Option`] is what accepts it.
//! - [`FromDialogueArgs`] reads the *whole* call. It is what a system input
//!   is, and it is implemented for tuples of up to eight [`FromMaybeValue`],
//!   for a single one, and for `()`.
//!
//! ```
//! # use bevy::prelude::*;
//! # use bevy_replique::prelude::*;
//! /// `>> play("bell")` and `>> play("bell", 0.5)` both fit.
//! fn play(In((sound, volume)): In<(String, Option<f32>)>) {
//!     let _ = (sound, volume.unwrap_or(1.0));
//! }
//! # let mut app = App::new();
//! # app.add_dialogue_command("play", play);
//! ```
//!
//! Two ways out when a tuple cannot say it. [`DialogueArgs`] takes the call
//! raw, arguments left as [`Value`], for a system whose arity is not fixed or
//! which needs the entity it was called from. Implementing [`FromDialogueArgs`]
//! yourself covers the rest: a call whose shape depends on its first argument.

use std::{any::type_name, fmt::Display};

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
/// **A dict argument.** `>> show({name: "Alice", hp: 12})` arrives as a
/// [`Value::Dict`], a map of names to values, which is how a call carries a
/// record rather than a flat list. There is no blanket reading of a dict into
/// a struct — which field goes where is yours to say — so a type that wants
/// one takes the map apart itself:
///
/// ```
/// # use bevy::prelude::*;
/// # use bevy_replique::prelude::*;
/// #[derive(Debug, PartialEq)]
/// struct Stats {
///     name: String,
///     hp: i64,
/// }
///
/// impl FromValue for Stats {
///     fn from_value(value: Value) -> Option<Self> {
///         let Value::Dict(mut fields) = value else {
///             return None;
///         };
///
///         // A key the dict does not hold is a missing field, and refuses
///         // the whole argument rather than standing in for a default.
///         Some(Self {
///             name: String::from_value(fields.remove("name")?)?,
///             hp: i64::from_value(fields.remove("hp")?)?,
///         })
///     }
/// }
///
/// /// `>> show({name: "Alice", hp: 12})`
/// ///
/// /// A type of your own reaches a system inside a tuple, `(Stats,)` for a
/// /// single one: [`FromDialogueArgs`] is implemented for the types of this
/// /// crate, and for tuples of anything that reads a value.
/// fn show(In((stats,)): In<(Stats,)>) {
///     let _ = stats;
/// }
/// # let mut app = App::new();
/// # app.add_dialogue_command("show", show);
/// #
/// # let dict = Value::Dict(std::collections::HashMap::from([
/// #     ("name".to_owned(), Value::String("Alice".into())),
/// #     ("hp".to_owned(), Value::Int(12)),
/// # ]));
/// # assert_eq!(
/// #     Stats::from_value(dict),
/// #     Some(Stats { name: "Alice".to_owned(), hp: 12 }),
/// # );
/// # assert_eq!(Stats::from_value(Value::Int(1)), None);
/// ```
///
/// The `bevy_custom_command` example reads one this way.
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
///     fn from_dialogue_args(args: DialogueArgs) -> Result<Self, DialogueArgsError> {
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
/// **A call whose arity is not fixed.** A tuple says how many arguments there
/// are, so it cannot take `>> add_scene(Alice)` and `>> add_scene(Alice, Bob)`
/// both. Reading [`args`](DialogueArgs::args) yourself can: the values come in
/// the order the writer put them, and [`FromValue`] reads each one, the index
/// of the loop being what makes the error point at the argument at fault.
///
/// ```
/// # use bevy::prelude::*;
/// # use bevy_replique::prelude::*;
/// /// `>> add_scene(Alice)` and `>> add_scene(Alice, Bob)` both fit.
/// struct Names(Vec<String>);
///
/// impl FromDialogueArgs for Names {
///     fn from_dialogue_args(args: DialogueArgs) -> Result<Self, DialogueArgsError> {
///         args.args
///             .into_iter()
///             .enumerate()
///             .map(|(index, value)| {
///                 String::from_value(value)
///                     .ok_or(DialogueArgsError::Argument { index, expected: "a name" })
///             })
///             .collect::<Result<Vec<_>, _>>()
///             .map(Self)
///     }
/// }
///
/// fn add_scene(In(Names(names)): In<Names>) {
///     let _ = names;
/// }
/// # let mut app = App::new();
/// # app.add_dialogue_command("add_scene", add_scene);
/// ```
///
/// [`DialogueArgs`] itself is the same thing without the typing, when the
/// values are better read one by one inside the system. The
/// `bevy_custom_command` example holds a full version of both.
///
/// A call that does not fit is logged and skipped, and the dialogue carries on:
/// the error you return is what the log shows.
pub trait FromDialogueArgs: Sized {
    fn from_dialogue_args(args: DialogueArgs) -> Result<Self, DialogueArgsError>;
}

impl FromDialogueArgs for DialogueArgs {
    fn from_dialogue_args(args: DialogueArgs) -> Result<Self, DialogueArgsError> {
        Ok(args)
    }
}

impl FromDialogueArgs for () {
    fn from_dialogue_args(args: DialogueArgs) -> Result<Self, DialogueArgsError> {
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

macro_rules! impl_from_dialogue_args {
    ($($T:ident),+) => {
        impl<$($T: FromMaybeValue),+> FromDialogueArgs for ($($T,)+) {
            #[allow(unused_assignments, non_snake_case)]
            fn from_dialogue_args(args: DialogueArgs) -> Result<Self, DialogueArgsError> {
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

impl_from_dialogue_args!(A);
impl_from_dialogue_args!(A, B);
impl_from_dialogue_args!(A, B, C);
impl_from_dialogue_args!(A, B, C, D);
impl_from_dialogue_args!(A, B, C, D, E);
impl_from_dialogue_args!(A, B, C, D, E, F);
impl_from_dialogue_args!(A, B, C, D, E, F, G);
impl_from_dialogue_args!(A, B, C, D, E, F, G, H);

macro_rules! impl_from_dialogue_args_single {
    ($($T:ty),*) => {$(
        impl FromDialogueArgs for $T {
            fn from_dialogue_args(args: DialogueArgs) -> Result<Self, DialogueArgsError> {
                <($T,)>::from_dialogue_args(args).map(|(value,)| value)
            }
        }
    )*};
}

impl_from_dialogue_args_single!(
    Value, String, bool, f32, f64, i8, i16, i32, i64, isize, u8, u16, u32, u64, usize
);

/// A Rust type written back as the [`Value`] a dialogue reads.
///
/// The mirror of [`FromValue`], for the answer of a function rather
/// than the argument of a call.
///
/// Implemented for [`Value`] itself, [`bool`], [`String`], the numbers, and
/// `HashMap<String, Value>`. Implementing it for a type of your own is what
/// lets a function hand one back:
///
/// ```
/// # use std::collections::HashMap;
/// # use bevy::prelude::*;
/// # use bevy_replique::prelude::*;
/// /// Read by the dialogue as `$stat.hp` and `$stat.gold`.
/// #[derive(Component, Clone, Copy)]
/// struct Stats {
///     hp: i64,
///     gold: i64,
/// }
///
/// impl IntoValue for Stats {
///     fn into_value(self) -> Value {
///         // A dict is how a value carries several named fields at once.
///         Value::Dict(HashMap::from([
///             ("hp".to_owned(), Value::Int(self.hp)),
///             ("gold".to_owned(), Value::Int(self.gold)),
///         ]))
///     }
/// }
///
/// /// `[let $stat = get_stat()]`, then `[if $stat.hp > 10]`
/// fn get_stat(In(()): In<()>, stats: Single<&Stats>) -> Stats {
///     **stats
/// }
/// # let mut app = App::new();
/// # app.add_dialogue_function("get_stat", get_stat);
/// #
/// # let value = Stats { hp: 12, gold: 3 }.into_value();
/// # assert!(matches!(value, Value::Dict(_)));
/// ```
pub trait IntoValue {
    fn into_value(self) -> Value;
}

impl IntoValue for bool {
    fn into_value(self) -> Value {
        Value::Bool(self)
    }
}

impl IntoValue for String {
    fn into_value(self) -> Value {
        Value::String(self)
    }
}

impl IntoValue for Value {
    fn into_value(self) -> Value {
        self
    }
}

impl IntoValue for std::collections::HashMap<String, Value> {
    fn into_value(self) -> Value {
        Value::Dict(self)
    }
}

macro_rules! impl_from_value_float {
    ($($T:ty),*) => {$(
        impl IntoValue for $T {
            fn into_value(self) -> Value {
                Value::Float(self as f64)
            }
        }
    )*};
}

impl_from_value_float!(f32, f64);

macro_rules! impl_from_value_int {
    ($($T:ty),*) => {$(
        impl IntoValue for $T {
            fn into_value(self) -> Value {
                Value::Int(self as i64)
            }
        }
    )*};
}

impl_from_value_int!(i8, i16, i32, i64, isize, u8, u16, u32, u64, usize);

/// What a function answers, once the system that runs it has returned.
///
/// Two shapes fit, and `M` is only there to tell them apart.
///
/// - any [`IntoValue`], for a function that always has an answer;
/// - `Result<T, E>` where `T: IntoValue` and `E: Display`, for one that may
///   not. The error reaches the log as the reason the call failed.
pub trait IntoFunctionOutput<M> {
    fn into_function_output(self) -> Result<Value, String>;
}

/// Marker of a function that always answers.
pub struct PlainOutput;

/// Marker of a function that answers a `Result`.
pub struct ResultOutput;

impl<T: IntoValue> IntoFunctionOutput<PlainOutput> for T {
    fn into_function_output(self) -> Result<Value, String> {
        Ok(self.into_value())
    }
}

impl<T: IntoValue, E: Display> IntoFunctionOutput<ResultOutput> for Result<T, E> {
    fn into_function_output(self) -> Result<Value, String> {
        self.map(IntoValue::into_value)
            .map_err(|err| err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A call of `args`, from a runner no test here looks at.
    fn args(args: Vec<Value>) -> DialogueArgs {
        DialogueArgs {
            runner: Entity::PLACEHOLDER,
            args,
        }
    }

    fn str(text: &str) -> Value {
        Value::String(text.to_owned())
    }

    #[test]
    fn a_tuple_reads_one_argument_per_field() {
        assert_eq!(
            <(String, f32, bool)>::from_dialogue_args(args(vec![
                str("bell"),
                Value::Float(0.5),
                Value::Bool(true),
            ])),
            Ok(("bell".to_owned(), 0.5, true)),
        );
    }

    #[test]
    fn a_single_argument_needs_no_tuple() {
        assert_eq!(
            String::from_dialogue_args(args(vec![str("hi")])),
            Ok("hi".to_owned())
        );
        assert_eq!(
            bool::from_dialogue_args(args(vec![Value::Bool(true)])),
            Ok(true)
        );
    }

    #[test]
    fn the_empty_tuple_takes_no_argument_at_all() {
        assert_eq!(<()>::from_dialogue_args(args(vec![])), Ok(()));
        assert_eq!(
            <()>::from_dialogue_args(args(vec![Value::Bool(true)])),
            Err(DialogueArgsError::TooManyArgs {
                expected: 0,
                got: 1
            }),
        );
    }

    /// An absent argument is the only thing an [`Option`] adds: a present one
    /// that does not fit is still an error.
    #[test]
    fn a_trailing_option_makes_an_argument_optional() {
        assert_eq!(
            <(String, Option<f32>)>::from_dialogue_args(args(vec![str("a")])),
            Ok(("a".to_owned(), None)),
        );
        assert_eq!(
            <(String, Option<f32>)>::from_dialogue_args(args(vec![str("b"), Value::Float(1.0)])),
            Ok(("b".to_owned(), Some(1.0))),
        );
        assert_eq!(
            <(String, Option<f32>)>::from_dialogue_args(args(vec![str("c"), Value::Bool(true)])),
            Err(DialogueArgsError::Argument {
                index: 1,
                expected: "Option<f32>",
            }),
        );
    }

    /// A number is read as the type it is written with: `3` and `3.0` are not
    /// the same argument, and neither is silently turned into the other.
    #[test]
    fn a_number_keeps_the_type_it_was_written_with() {
        assert_eq!(f32::from_value(Value::Float(3.0)), Some(3.0));
        assert_eq!(f32::from_value(Value::Int(3)), None);
        assert_eq!(i64::from_value(Value::Int(3)), Some(3));
        assert_eq!(i64::from_value(Value::Float(3.0)), None);
    }

    /// `Value` and `DialogueArgs` are conversions too, the identity ones: they
    /// are how a command takes what the dialogue wrote, untouched.
    #[test]
    fn a_raw_value_passes_through_untouched() {
        assert_eq!(Value::from_value(str("a")), Some(str("a")));

        let call = args(vec![str("a"), Value::Int(1)]);
        assert_eq!(DialogueArgs::from_dialogue_args(call.clone()), Ok(call));
    }

    #[test]
    fn a_type_of_your_own_composes_into_a_tuple() {
        #[derive(Debug, PartialEq)]
        struct Direction(String);

        impl FromValue for Direction {
            fn from_value(value: Value) -> Option<Self> {
                match String::from_value(value)?.as_str() {
                    word @ ("left" | "right") => Some(Self(word.to_owned())),
                    _ => None,
                }
            }
        }

        assert_eq!(
            <(Direction, Option<f32>)>::from_dialogue_args(args(vec![str("left")])),
            Ok((Direction("left".to_owned()), None)),
        );
        // A word the type does not know is a bad argument, not a panic.
        assert_eq!(
            <(Direction,)>::from_dialogue_args(args(vec![str("up")])),
            Err(DialogueArgsError::Argument {
                index: 0,
                expected: "Direction",
            }),
        );
    }

    #[test]
    fn error_on_an_argument_that_is_missing_or_of_the_wrong_type() {
        assert_eq!(
            <(String, f32)>::from_dialogue_args(args(vec![str("a")])),
            Err(DialogueArgsError::Argument {
                index: 1,
                expected: "f32",
            }),
        );
        assert_eq!(
            <(String,)>::from_dialogue_args(args(vec![Value::Bool(true)])),
            Err(DialogueArgsError::Argument {
                index: 0,
                expected: "String",
            }),
        );
    }

    #[test]
    fn error_on_more_arguments_than_the_signature_holds() {
        assert_eq!(
            <(String,)>::from_dialogue_args(args(vec![str("a"), Value::Bool(true)])),
            Err(DialogueArgsError::TooManyArgs {
                expected: 1,
                got: 2
            }),
        );
    }
}
