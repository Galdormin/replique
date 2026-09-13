extern crate proc_macro;
extern crate syn;
#[macro_use]
extern crate quote;

use proc_macro::TokenStream;
use syn::{Data, DeriveInput, parse_macro_input};

mod value;

/// Implements both `FromValue` and `IntoValue`, so the type is a vocabulary the
/// dialogue and the game share.
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
/// // A word the enum does not know is refused, spelling included.
/// assert_eq!(Character::from_value(Value::String("JeanMichel".to_owned())), None);
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
///   differs from its name.
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
///     Some(Stats { name: "Alice".to_owned(), health: 12, speed: 18.36 }),
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
/// assert_eq!(Volume::from_value(Value::Float(0.8)), Some(Volume(0.8)));
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
