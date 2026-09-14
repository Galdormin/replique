extern crate proc_macro;
extern crate syn;
#[macro_use]
extern crate quote;

use proc_macro::TokenStream;
use syn::{Data, DeriveInput, parse_macro_input};

mod args;
mod attrs;
mod call;
mod value;

/// Implements both `FromValue` and `IntoValue`, so the type is a vocabulary the
/// dialogue and the game share. A value that does not fit is refused with a
/// `ValueError` saying what was expected and, inside a dict, under which key.
///
/// Three shapes derive it, each mapping to one way of writing a value:
///
/// | Shape | Written in the dialogue as |
/// | --- | --- |
/// | Unit enum | a string, `Alice` |
/// | Named struct | a dict, `{name: "Alice", hp: 12}` |
/// | Single value unnamed struct | whatever the field it wraps is |
///
/// # Unit enum
///
/// A variant is the word the dialogue writes, matched as it is spelled.
/// `#[replique(alias = "...")]` gives a variant another spelling, which is what
/// a writer typing lowercase needs.
///
/// ```
/// use bevy_replique::prelude::*;
///
/// /// `>> hurt(Alice, 8)`
/// #[derive(RepliqueValue, Debug, PartialEq)]
/// enum Character {
///     Alice,
///     Bob,
///     #[replique(alias="Jean-Michel")]
///     JeanMichel
/// }
///
/// assert_eq!(Character::Alice.into_value(), Value::String("Alice".to_owned()));
/// assert_eq!(Character::JeanMichel.into_value(), Value::String("Jean-Michel".to_owned()));
///
/// assert_eq!(
///     Character::from_value(Value::String("Jean-Michel".to_owned())),
///     Ok(Character::JeanMichel),
/// );
///
/// // A word the enum does not know is refused, spelling included, and the
/// // log names the word that was written.
/// assert_eq!(
///     Character::from_value(Value::String("JeanMichel".to_owned()))
///         .unwrap_err()
///         .to_string(),
///     "`JeanMichel` is not a `Character`",
/// );
/// ```
///
/// A variant holding data cannot derive it: only a unit enum can.
///
/// # Named struct
///
/// A field is a key of the dict, and every one of them has to be written: a
/// missing key is a refused value, and a field of type `Option` is turned down
/// rather than read as optional.
///
/// Three attributes, on a field:
///
/// - `#[replique(alias = "hp")]` — the key the field answers to, when it
///   differs from its name. It is the name the errors use, being the one the
///   dialogue writes.
/// - `#[replique(ignore)]` — the field is no key at all. It is left out of the
///   dict written back, and takes its value from [`Default`], which the struct
///   then has to implement.
/// - `#[replique(ignore, default = 18.36)]` — an ignored field taking the value
///   of that expression instead, which asks nothing of the rest of the struct.
///
/// ```
/// use bevy_replique::prelude::*;
///
/// /// `>> spawn({name: "Alice", hp: 12})`
/// #[derive(RepliqueValue, Debug, PartialEq)]
/// struct Stats {
///     name: String,
///     #[replique(alias = "hp")]
///     health: i64,
///     #[replique(ignore, default = 18.36)]
///     speed: f64,
/// }
///
/// let dict = Value::Dict(std::collections::HashMap::from([
///     ("name".to_owned(), Value::String("Alice".to_owned())),
///     ("hp".to_owned(), Value::Int(12)),
/// ]));
/// assert_eq!(
///     Stats::from_value(dict),
///     Ok(Stats { name: "Alice".to_owned(), health: 12, speed: 18.36 }),
/// );
///
/// // A key left out, or holding the wrong type, is named in the error.
/// let short = Value::Dict(std::collections::HashMap::from([
///     ("name".to_owned(), Value::String("Alice".to_owned())),
/// ]));
/// assert_eq!(
///     Stats::from_value(short).unwrap_err().to_string(),
///     "key `hp`: missing",
/// );
///
/// // `speed` is no key: it is neither read nor written.
/// assert_eq!(
///     Stats { name: "Alice".to_owned(), health: 12, speed: 0.0 }.into_value(),
///     Value::Dict(std::collections::HashMap::from([
///         ("name".to_owned(), Value::String("Alice".to_owned())),
///         ("hp".to_owned(), Value::Int(12)),
///     ])),
/// );
/// ```
///
/// # Single value unnamed struct
///
/// A struct wrapping one value is that value: it is read and written as the
/// type it holds, which is the way to give a name of your own to a number the
/// dialogue writes bare. No attribute applies, and a tuple struct holding more
/// than one field cannot derive it.
///
/// ```
/// use bevy_replique::prelude::*;
///
/// /// `>> set_volume(0.8)`
/// #[derive(RepliqueValue, Debug, PartialEq)]
/// struct Volume(f64);
///
/// assert_eq!(Volume::from_value(Value::Float(0.8)), Ok(Volume(0.8)));
/// assert_eq!(Volume(0.8).into_value(), Value::Float(0.8));
/// ```
#[proc_macro_derive(RepliqueValue, attributes(replique))]
pub fn replique_value(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = input.ident;

    match input.data {
        Data::Struct(data_struct) => value::replique_value_struct(&name, &data_struct),
        Data::Enum(data_enum) => value::replique_value_enum(&name, &data_enum),
        Data::Union(_) => TokenStream::from(
            syn::Error::new(
                name.span(),
                "Only enum or struct can derive `RepliqueValue`",
            )
            .to_compile_error(),
        ),
    }
}

/// Implements `FromDialogueArgs`, for the calls a plain tuple cannot say.
/// A signature that lines up one argument per field needs none of this:
/// `In<(String, f32)>` already is that, and a trailing `Option` already makes
/// the last argument optional. This derive is for the three shapes left over.
///
/// **A call that dispatches on its first word.** An enum reads the word the
/// call starts with, and each variant says what follows it, so one command
/// covers several shapes of its own arity. A variant holds its arguments in
/// the order it writes them, named or unnamed alike, and a unit one takes
/// none; `#[replique(alias = "...")]` gives a variant another spelling, as it
/// does in [`RepliqueValue`].
///
/// ```
/// # use bevy::prelude::*;
/// use bevy_replique::prelude::*;
///
/// /// `>> camera(shake)`, `>> camera(move, 120, 40)`
/// #[derive(RepliqueArgs, Debug, PartialEq)]
/// enum Camera {
///     #[replique(alias = "shake")]
///     Shake,
///     #[replique(alias = "move")]
///     MoveTo { x: f32, y: f32 },
/// }
///
/// # fn call(args: Vec<Value>) -> DialogueArgs {
/// #     DialogueArgs { runner: Entity::PLACEHOLDER, args }
/// # }
/// fn move_camera(In(order): In<Camera>) {
///     let _ = order;
/// }
/// # let mut app = App::new();
/// # app.add_dialogue_command_named("camera", move_camera);
/// #
/// # assert_eq!(
/// #     Camera::from_dialogue_args(call(vec![Value::String("shake".to_owned())])),
/// #     Ok(Camera::Shake),
/// # );
/// // A word no variant answers to is refused, and named in the log.
/// assert_eq!(
///     Camera::from_dialogue_args(call(vec![Value::String("zoom".to_owned())]))
///         .unwrap_err()
///         .to_string(),
///     "argument 0: `zoom` is not a `Camera`",
/// );
/// ```
///
/// **A call that needs the dialogue it came from.** `#[runner]` on a field
/// hands it the entity holding the `DialogueRunner`, which matters as soon as
/// two dialogues run at once. It reads no argument, so it takes no position:
/// the fields around it are numbered as if it were not there.
///
/// ```
/// # use bevy::prelude::*;
/// use bevy_replique::prelude::*;
///
/// /// `>> lock(12)`
/// #[derive(RepliqueArgs)]
/// struct Lock {
///     #[runner]
///     runner: Entity,
///     seconds: f32,
/// }
///
/// #[replique_command]
/// fn lock(In(Lock { runner, seconds }): In<Lock>, mut commands: Commands) {
///     let _ = (commands.entity(runner), seconds);
/// }
/// # let mut app = App::new();
/// # app.add_dialogue_command(lock);
/// ```
///
/// **A call whose arity is not fixed.** `#[variadic]` on a last field collects
/// every argument left into it, so `>> add_scene(Alice)` and
/// `>> add_scene(Alice, Bob)` both fit. Taking none of them is a call too: the
/// collection is then empty rather than refused. Fixed fields may come before
/// it, and the index an error names is the one the writer sees, not the one
/// inside the collection.
///
/// ```
/// # use bevy::prelude::*;
/// use bevy_replique::prelude::*;
///
/// /// `>> notify("saved")` and `>> notify("hit", 1, 2)`
/// #[derive(RepliqueArgs, Debug, PartialEq)]
/// struct Notify(String, #[variadic] Vec<i64>);
///
/// # fn call(args: Vec<Value>) -> DialogueArgs {
/// #     DialogueArgs { runner: Entity::PLACEHOLDER, args }
/// # }
///
/// #[replique_command]
/// fn notify(In(Notify(text, targets)): In<Notify>) {
///     let _ = (text, targets);
/// }
/// # let mut app = App::new();
/// # app.add_dialogue_command(notify);
/// #
/// # assert_eq!(
/// #     Notify::from_dialogue_args(call(vec![Value::String("saved".to_owned())])),
/// #     Ok(Notify("saved".to_owned(), vec![])),
/// # );
/// // The third argument is named as the third, not as the second of the `Vec`.
/// assert_eq!(
///     Notify::from_dialogue_args(call(vec![
///         Value::String("hit".to_owned()),
///         Value::Int(1),
///         Value::Bool(true),
///     ]))
///     .unwrap_err()
///     .to_string(),
///     "argument 2: expected int, got bool",
/// );
/// ```
///
/// A field of type `Option` is turned down in a shape that has a `#[variadic]`
/// one: an optional argument left out and a first variadic one are written the
/// same, and nothing in the call tells them apart.
#[proc_macro_derive(RepliqueArgs, attributes(replique, runner, variadic))]
pub fn replique_args(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = input.ident;

    match input.data {
        Data::Struct(data_struct) => args::replique_args_struct(&name, &data_struct),
        Data::Enum(data_enum) => args::replique_args_enum(&name, &data_enum),
        Data::Union(_) => TokenStream::from(
            syn::Error::new(name.span(), "Only enum or struct can derive `RepliqueArgs`")
                .to_compile_error(),
        ),
    }
}

/// Declares a system as a function the dialogue can call.
///
/// Writes the name the dialogue calls it by next to what it does, so that the
/// registration has nothing left to repeat: `app.add_dialogue_function(upper)`.
///
/// ```
/// # use bevy::prelude::*;
/// # use bevy_replique::prelude::*;
/// /// The name in upper case, to shout it.
/// #[replique_function(name = "upper")]
/// fn shout(In((text,)): In<(String,)>) -> String {
///     text.to_uppercase()
/// }
///
/// # let mut app = App::new();
/// app.add_dialogue_function(shout);
/// assert_eq!(shout::NAME, "upper");
/// assert_eq!(shout::DOC, "The name in upper case, to shout it.");
/// ```
///
/// Without `name`, the system is called by its own name. The documentation
/// written over it is kept, reachable as `shout::DOC`.
///
/// The system itself moves under `shout::system`.
///
/// # The name becomes a type
///
/// What the attribute leaves behind under the name of the system is a unit
/// struct, and a unit struct is a pattern. That name can therefore no longer
/// be bound to anything in the crate — not a parameter, not a `let`, not a
/// field of a destructuring — and the compiler says so wherever it is tried:
///
/// ```text
/// error[E0530]: function parameters cannot shadow unit structs
///    |
///  3 | #[replique_function]
///    | -------------------- the unit struct `gold` is defined here
/// ...
/// 12 | fn set_gold(In((amount,)): In<(i64,)>, mut gold: ResMut<Gold>) {
///    |                                            ^^^^ cannot be named the same as a unit struct
/// ```
///
/// It happens more often than it sounds: a dialogue calls a function by the
/// name of the thing it is about, and so does the resource holding that thing.
/// Two ways out, both fine.
///
/// **Rename what is bound.** The system keeps the name the dialogue uses, and
/// the parameter takes another:
///
/// ```
/// # use bevy::prelude::*;
/// # use bevy_replique::prelude::*;
/// # #[derive(Resource)]
/// # struct Gold(i64);
/// #[replique_function]
/// fn gold(In(()): In<()>, purse: Res<Gold>) -> i64 {
///     purse.0
/// }
/// ```
///
/// **Or rename the system and say the dialogue name in the attribute.** A
/// prefix keeps the Rust side out of the way and reads well in a file that
/// holds a lot of them:
///
/// ```
/// # use bevy::prelude::*;
/// # use bevy_replique::prelude::*;
/// # #[derive(Resource)]
/// # struct Gold(i64);
/// #[replique_function(name = "gold")]
/// fn fn_gold(In(()): In<()>, gold: Res<Gold>) -> i64 {
///     gold.0
/// }
/// # assert_eq!(fn_gold::NAME, "gold");
/// ```
///
/// The second is worth taking as a habit if the collision bites twice: what
/// the dialogue writes stays in the attribute, where it is declared once, and
/// the Rust names stop competing with it. `cmd_` does the same for
/// [`macro@replique_command`].
#[proc_macro_attribute]
pub fn replique_function(args: TokenStream, item: TokenStream) -> TokenStream {
    call::expand(call::Kind::Function, args, item)
}

/// Declares a system as a `>>` command of the dialogue.
///
/// The same as [`macro@replique_function`], for a system that runs rather than
/// answers: it writes the word the dialogue puts after the `>>` next to what it
/// does, so that `app.add_dialogue_command(set_flag)` has no name to repeat.
///
/// ```
/// # use bevy::prelude::*;
/// # use bevy_replique::prelude::*;
/// /// Turns a flag of the save on or off.
/// #[replique_command(name = "set_flag")]
/// fn raise(In((flag, on)): In<(String, bool)>) {
///     let _ = (flag, on);
/// }
///
/// # let mut app = App::new();
/// app.add_dialogue_command(raise);
/// assert_eq!(raise::NAME, "set_flag");
/// ```
///
/// The name of the system becomes a unit struct here too, so it can no longer
/// be bound to anything in the crate. See [the section on
/// it](macro@replique_function#the-name-becomes-a-type) for the two ways
/// around; `cmd_set_flag` with `name = "set_flag"` is the one that scales.
#[proc_macro_attribute]
pub fn replique_command(args: TokenStream, item: TokenStream) -> TokenStream {
    call::expand(call::Kind::Command, args, item)
}
