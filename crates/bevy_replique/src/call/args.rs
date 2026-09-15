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
//! A call that does not fit is logged and skipped, and the dialogue carries
//! on. What the log says is a [`ValueError`], worded in the terms of the
//! dialogue rather than of Rust, wrapped in a [`DialogueArgsError`] naming the
//! argument it came from: `argument 0: key `hp`: expected int, got string`.
//!
//! ```
//! # use bevy::prelude::*;
//! # use bevy_replique::prelude::*;
//! /// `>> play("bell")` and `>> play("bell", 0.5)` both fit.
//! fn play(In((sound, volume)): In<(String, Option<f32>)>) {
//!     let _ = (sound, volume.unwrap_or(1.0));
//! }
//! # let mut app = App::new();
//! # app.add_dialogue_command_named("play", play);
//! ```
//!
//! Two ways out when a tuple cannot say it. [`DialogueArgs`] takes the call
//! raw, arguments left as [`Value`], for a system whose arity is not fixed or
//! which needs the entity it was called from. Implementing [`FromDialogueArgs`]
//! yourself covers the rest: a call whose shape depends on its first argument.

use std::fmt::Display;

use replique::dialogue::{Value, ValueType};
use thiserror::Error;

/// What a failed [`FromValue`] says. It is worded in the terms the dialogue is
/// written in rather than in Rust ones — `dict` and `int`, not `Stats` and
/// `i64` — because the log it ends up in is read by whoever wrote the line
/// that failed.
///
/// [`Key`](ValueError::Key) is what makes it say *where*: a type reading a
/// dict puts whatever went wrong back under the key it went wrong in, so a
/// failure several levels down still names the whole path.
#[derive(Error, Debug, PartialEq)]
pub enum ValueError {
    /// Nothing written where a value was expected.
    #[error("missing")]
    Missing,
    /// A value of the wrong type.
    #[error("expected {expected}, got {got}")]
    Type {
        /// The type as the dialogue names it, `"dict"` rather than `Stats`.
        expected: &'static str,
        got: ValueType,
    },
    /// A word outside the ones a type knows, `>> face(up)` for a `Direction`
    /// that is `left` or `right`.
    #[error("`{got}` is not a `{expected}`")]
    Unknown { expected: &'static str, got: String },
    /// Something under a key of a dict, named by that key.
    #[error("key `{key}`: {source}")]
    Key {
        key: String,
        source: Box<ValueError>,
    },
}

impl ValueError {
    /// Put a failure back under the key it happened in, on the way out of a
    /// dict. This is what builds the path an error names.
    ///
    /// ```
    /// # use bevy_replique::prelude::*;
    /// let err = ValueError::Type { expected: "int", got: ValueType::String };
    /// assert_eq!(
    ///     err.under_key("hp").to_string(),
    ///     "key `hp`: expected int, got string",
    /// );
    /// ```
    pub fn under_key(self, key: impl Into<String>) -> Self {
        Self::Key {
            key: key.into(),
            source: Box::new(self),
        }
    }

    /// A value that is not of the type asked for, `expected` being that type
    /// as the dialogue names it.
    pub fn wrong_type(expected: &'static str, value: &Value) -> Self {
        Self::Type {
            expected,
            got: ValueType::type_of(value),
        }
    }
}

/// A call that does not fit the signature the system asked for.
#[derive(Error, Debug, PartialEq)]
pub enum DialogueArgsError {
    #[error("expected at most {expected} argument(s), got {got}")]
    TooManyArgs { expected: usize, got: usize },
    #[error("argument {index}: {source}")]
    Argument { index: usize, source: ValueError },
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
///     for value in &args.0 {
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
/// # app.add_dialogue_command_named("add", add);
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct DialogueArgs(pub Vec<Value>);

/// One argument, read into the type a command system asked for.
///
/// An argument the dialogue did not write never reaches it: that case belongs
/// to [`FromMaybeValue`], which is what a trailing [`Option`] hooks into.
///
/// Implementing it on a type of your own is the usual way to teach the
/// dialogue a new vocabulary: the type then composes into a tuple like any
/// other, and a word the enum does not know is reported as a bad argument
/// instead of reaching the game. What it returns on the way out is a
/// [`ValueError`], which is what the log shows — so it is worth saying which
/// value was wrong rather than only that one was.
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
///     fn from_value(value: Value) -> Result<Self, ValueError> {
///         // Bare words and quoted text are both strings, so `left` and
///         // `"left"` are the same argument.
///         let word = String::from_value(value)?;
///         match word.as_str() {
///             "left" => Ok(Self::Left),
///             "right" => Ok(Self::Right),
///             _ => Err(ValueError::Unknown { expected: "Direction", got: word }),
///         }
///     }
/// }
///
/// /// `>> face(left)` and `>> face("right", 0.5)` both fit.
/// fn face(In((direction, speed)): In<(Direction, Option<f32>)>) {
///     let _ = (direction, speed.unwrap_or(1.0));
/// }
/// # let mut app = App::new();
/// # app.add_dialogue_command_named("face", face);
/// #
/// # assert_eq!(
/// #     Direction::from_value(Value::String("left".into())),
/// #     Ok(Direction::Left),
/// # );
/// assert_eq!(
///     Direction::from_value(Value::String("up".into()))
///         .unwrap_err()
///         .to_string(),
///     "`up` is not a `Direction`",
/// );
/// ```
///
/// A type this shape is what `#[derive(RepliqueValue)]` writes for you.
///
/// **A dict argument.** `>> show({name: "Alice", hp: 12})` arrives as a
/// [`Value::Dict`], a map of names to values, which is how a call carries a
/// record rather than a flat list. There is no blanket reading of a dict into
/// a struct — which field goes where is yours to say — so a type that wants
/// one takes the map apart itself, and names the key each failure is under:
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
///     fn from_value(value: Value) -> Result<Self, ValueError> {
///         let Value::Dict(mut fields) = value else {
///             return Err(ValueError::wrong_type("dict", &value));
///         };
///
///         // A key the dict does not hold is a missing field, and refuses
///         // the whole argument rather than standing in for a default.
///         // `under_key` is what puts the name of the key in the log.
///         Ok(Self {
///             name: String::from_maybe_value(fields.remove("name"))
///                 .map_err(|err| err.under_key("name"))?,
///             hp: i64::from_maybe_value(fields.remove("hp"))
///                 .map_err(|err| err.under_key("hp"))?,
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
/// # app.add_dialogue_command_named("show", show);
/// #
/// # let dict = Value::Dict(std::collections::HashMap::from([
/// #     ("name".to_owned(), Value::String("Alice".into())),
/// #     ("hp".to_owned(), Value::Int(12)),
/// # ]));
/// # assert_eq!(
/// #     Stats::from_value(dict),
/// #     Ok(Stats { name: "Alice".to_owned(), hp: 12 }),
/// # );
/// // A dict short of a key says which one, instead of refusing the whole
/// // argument without a reason.
/// let dict = Value::Dict(std::collections::HashMap::from([
///     ("name".to_owned(), Value::String("Alice".into())),
/// ]));
/// assert_eq!(
///     Stats::from_value(dict).unwrap_err().to_string(),
///     "key `hp`: missing",
/// );
/// assert_eq!(
///     Stats::from_value(Value::Int(1)).unwrap_err().to_string(),
///     "expected dict, got int",
/// );
/// ```
///
/// The `bevy_custom_command` example reads one this way.
pub trait FromValue: Sized {
    fn from_value(value: Value) -> Result<Self, ValueError>;
}

impl FromValue for Value {
    fn from_value(value: Value) -> Result<Self, ValueError> {
        Ok(value)
    }
}

impl FromValue for String {
    fn from_value(value: Value) -> Result<Self, ValueError> {
        match value {
            Value::String(s) => Ok(s),
            other => Err(ValueError::wrong_type("string", &other)),
        }
    }
}

impl FromValue for bool {
    fn from_value(value: Value) -> Result<Self, ValueError> {
        match value {
            Value::Bool(b) => Ok(b),
            other => Err(ValueError::wrong_type("bool", &other)),
        }
    }
}

/// One argument, present or not.
///
/// Nothing to implement: every [`FromValue`] gets it, and refuses an argument
/// that is not there. [`Option`] is what accepts the absence, and that is the
/// whole of what a trailing `Option` in a signature means.
pub trait FromMaybeValue: Sized {
    fn from_maybe_value(value: Option<Value>) -> Result<Self, ValueError>;
}

impl<T: FromValue> FromMaybeValue for T {
    fn from_maybe_value(value: Option<Value>) -> Result<Self, ValueError> {
        T::from_value(value.ok_or(ValueError::Missing)?)
    }
}

/// An absent argument is the only thing an [`Option`] adds: a present one that
/// does not fit is still an error.
impl<T: FromValue> FromMaybeValue for Option<T> {
    fn from_maybe_value(value: Option<Value>) -> Result<Self, ValueError> {
        match value {
            None => Ok(None),
            Some(value) => T::from_value(value).map(Some),
        }
    }
}

macro_rules! impl_from_value_float {
    ($($T:ty),*) => {$(
        impl FromValue for $T {
            fn from_value(value: Value) -> Result<Self, ValueError> {
                match value {
                    Value::Float(f) => Ok(f as $T),
                    other => Err(ValueError::wrong_type("float", &other)),
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
            fn from_value(value: Value) -> Result<Self, ValueError> {
                match value {
                    Value::Int(i) => Ok(i as $T),
                    other => Err(ValueError::wrong_type("int", &other)),
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
///         // The index makes the log point at the argument that is wrong,
///         // and the `ValueError` says what is wrong with it.
///         let at = |index| move |source| DialogueArgsError::Argument { index, source };
///
///         // The arguments are read one by one, with the same
///         // `FromMaybeValue` the tuples use: absent is refused here.
///         let mut values = args.0.into_iter();
///         let order = String::from_maybe_value(values.next()).map_err(at(0))?;
///
///         match order.as_str() {
///             "shake" => Ok(Self::Shake),
///             "move" => Ok(Self::MoveTo {
///                 x: f32::from_maybe_value(values.next()).map_err(at(1))?,
///                 y: f32::from_maybe_value(values.next()).map_err(at(2))?,
///             }),
///             _ => Err(at(0)(ValueError::Unknown {
///                 expected: "Camera",
///                 got: order,
///             })),
///         }
///     }
/// }
///
/// fn move_camera(In(order): In<Camera>) {
///     let _ = order;
/// }
/// # let mut app = App::new();
/// # app.add_dialogue_command_named("camera", move_camera);
/// ```
///
/// **A call whose arity is not fixed.** A tuple says how many arguments there
/// are, so it cannot take `>> add_scene(Alice)` and `>> add_scene(Alice, Bob)`
/// both. Reading [`DialogueArgs`] yourself can: the values come in
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
///         args.0
///             .into_iter()
///             .enumerate()
///             .map(|(index, value)| {
///                 String::from_value(value)
///                     .map_err(|source| DialogueArgsError::Argument { index, source })
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
/// # app.add_dialogue_command_named("add_scene", add_scene);
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
        match args.0.len() {
            0 => Ok(()),
            got => Err(DialogueArgsError::TooManyArgs { expected: 0, got }),
        }
    }
}

macro_rules! impl_from_dialogue_args {
    ($($T:ident),+) => {
        impl<$($T: FromMaybeValue),+> FromDialogueArgs for ($($T,)+) {
            #[allow(unused_assignments, non_snake_case)]
            fn from_dialogue_args(args: DialogueArgs) -> Result<Self, DialogueArgsError> {
                let expected = [$(stringify!($T)),+].len();
                if args.0.len() > expected {
                    return Err(DialogueArgsError::TooManyArgs { expected, got: args.0.len() });
                }

                let mut values = args.0.into_iter();
                let mut index = 0;
                $(
                    let $T = $T::from_maybe_value(values.next()).map_err(
                        |source| DialogueArgsError::Argument { index, source },
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
/// # app.add_dialogue_function_named("get_stat", get_stat);
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
        DialogueArgs(args)
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
            <(String, Option<f32>)>::from_dialogue_args(args(vec![str("c"), Value::Bool(true)]))
                .unwrap_err()
                .to_string(),
            "argument 1: expected float, got bool",
        );
    }

    /// A number is read as the type it is written with: `3` and `3.0` are not
    /// the same argument, and neither is silently turned into the other.
    #[test]
    fn a_number_keeps_the_type_it_was_written_with() {
        assert_eq!(f32::from_value(Value::Float(3.0)), Ok(3.0));
        assert_eq!(i64::from_value(Value::Int(3)), Ok(3));
        assert_eq!(
            f32::from_value(Value::Int(3)),
            Err(ValueError::Type {
                expected: "float",
                got: ValueType::Int,
            }),
        );
        assert_eq!(
            i64::from_value(Value::Float(3.0)),
            Err(ValueError::Type {
                expected: "int",
                got: ValueType::Float,
            }),
        );
    }

    /// `Value` and `DialogueArgs` are conversions too, the identity ones: they
    /// are how a command takes what the dialogue wrote, untouched.
    #[test]
    fn a_raw_value_passes_through_untouched() {
        assert_eq!(Value::from_value(str("a")), Ok(str("a")));

        let call = args(vec![str("a"), Value::Int(1)]);
        assert_eq!(DialogueArgs::from_dialogue_args(call.clone()), Ok(call));
    }

    #[test]
    fn a_type_of_your_own_composes_into_a_tuple() {
        #[derive(Debug, PartialEq)]
        struct Direction(String);

        impl FromValue for Direction {
            fn from_value(value: Value) -> Result<Self, ValueError> {
                let word = String::from_value(value)?;
                match word.as_str() {
                    "left" | "right" => Ok(Self(word)),
                    _ => Err(ValueError::Unknown {
                        expected: "Direction",
                        got: word,
                    }),
                }
            }
        }

        assert_eq!(
            <(Direction, Option<f32>)>::from_dialogue_args(args(vec![str("left")])),
            Ok((Direction("left".to_owned()), None)),
        );
        // A word the type does not know is a bad argument, not a panic, and
        // the log names the word.
        assert_eq!(
            <(Direction,)>::from_dialogue_args(args(vec![str("up")]))
                .unwrap_err()
                .to_string(),
            "argument 0: `up` is not a `Direction`",
        );
    }

    /// An argument that is absent and one that is of the wrong type are two
    /// different failures, and the log tells them apart.
    #[test]
    fn error_on_an_argument_that_is_missing_or_of_the_wrong_type() {
        assert_eq!(
            <(String, f32)>::from_dialogue_args(args(vec![str("a")])),
            Err(DialogueArgsError::Argument {
                index: 1,
                source: ValueError::Missing,
            }),
        );
        assert_eq!(
            <(String,)>::from_dialogue_args(args(vec![Value::Bool(true)]))
                .unwrap_err()
                .to_string(),
            "argument 0: expected string, got bool",
        );
    }

    /// A dict read inside a dict keeps the whole path in the message, which
    /// is the point of reporting a `ValueError` rather than a bare failure.
    #[test]
    fn an_error_under_a_key_names_the_key() {
        #[derive(Debug, PartialEq)]
        struct Stats {
            hp: i64,
        }

        impl FromValue for Stats {
            fn from_value(value: Value) -> Result<Self, ValueError> {
                let Value::Dict(mut fields) = value else {
                    return Err(ValueError::wrong_type("dict", &value));
                };

                Ok(Self {
                    hp: i64::from_maybe_value(fields.remove("hp"))
                        .map_err(|err| err.under_key("hp"))?,
                })
            }
        }

        let dict = |value| Value::Dict(std::collections::HashMap::from([("hp".to_owned(), value)]));

        assert_eq!(
            Stats::from_value(dict(Value::Int(12))),
            Ok(Stats { hp: 12 })
        );
        assert_eq!(
            <(Stats,)>::from_dialogue_args(args(vec![dict(str("a"))]))
                .unwrap_err()
                .to_string(),
            "argument 0: key `hp`: expected int, got string",
        );
        assert_eq!(
            <(Stats,)>::from_dialogue_args(args(vec![Value::Dict(Default::default())]))
                .unwrap_err()
                .to_string(),
            "argument 0: key `hp`: missing",
        );
        assert_eq!(
            <(Stats,)>::from_dialogue_args(args(vec![Value::Int(1)]))
                .unwrap_err()
                .to_string(),
            "argument 0: expected dict, got int",
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
