//! The functions every dialogue can call, whatever the host calling the VM.
//!
//! They are pure. The `fn` pointer a [`Builtin`] holds cannot capture anything,
//! so a call depends on nothing but its arguments.

use std::fmt;

use thiserror::Error;

use crate::dialogue::{Value, ValueType};

pub const BUILTINS: &[Builtin] = &[
    Builtin::new(
        "upper",
        Arity::Exact(1),
        "Upper case of a string. `upper(\"bell\")` is `\"BELL\"`.",
        upper,
    ),
    Builtin::new(
        "lower",
        Arity::Exact(1),
        "Lower case of a string. `lower(\"Bell\")` is `\"bell\"`.",
        lower,
    ),
    Builtin::new(
        "capitalize",
        Arity::Exact(1),
        "Capitalize a string. `lower(\"bell\")` is `\"Bell\"`.",
        capitalize,
    ),
    Builtin::new(
        "len",
        Arity::Exact(1),
        "Characters of a string, or keys of a dict. `len(\"été\")` is `3`.",
        len,
    ),
    Builtin::new(
        "has",
        Arity::Exact(2),
        "Whether a dict holds a key. `has($stat, \"hp\")`.",
        has,
    ),
    Builtin::new(
        "abs",
        Arity::Exact(1),
        "Absolute value, of the type it is given. `abs(-3)` is `3`.",
        abs,
    ),
    Builtin::new(
        "min",
        Arity::AtLeast(2),
        "Smallest of the numbers, which have to be of one type.",
        min,
    ),
    Builtin::new(
        "max",
        Arity::AtLeast(2),
        "Largest of the numbers, which have to be of one type.",
        max,
    ),
    Builtin::new(
        "round",
        Arity::Range(1, 2),
        "Round a float to the given precision. `round(3.551, 2)` is `3.55`.",
        round,
    ),
    Builtin::new(
        "floor",
        Arity::Exact(1),
        "Largest whole float under this one. `floor(3.7)` is `3.0`.",
        floor,
    ),
    Builtin::new(
        "ceil",
        Arity::Exact(1),
        "Smallest whole float over this one. `ceil(3.2)` is `4.0`.",
        ceil,
    ),
];

/// The builtin that answers to `name`, if one does.
pub fn lookup(name: &str) -> Option<&'static Builtin> {
    BUILTINS.iter().find(|builtin| builtin.name == name)
}

/// How many arguments a builtin takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arity {
    Exact(usize),
    Range(usize, usize),
    AtLeast(usize),
}

impl Arity {
    pub fn accepts(&self, got: usize) -> bool {
        match *self {
            Arity::Exact(n) => got == n,
            Arity::Range(low, high) => (low..=high).contains(&got),
            Arity::AtLeast(n) => got >= n,
        }
    }
}

impl fmt::Display for Arity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Arity::Exact(1) => f.write_str("1 argument"),
            Arity::Exact(n) => write!(f, "{n} arguments"),
            Arity::Range(low, high) => write!(f, "{low} to {high} arguments"),
            Arity::AtLeast(n) => write!(f, "at least {n} arguments"),
        }
    }
}

/// An error in the call of a builtin
#[derive(Error, Debug, Clone, PartialEq)]
pub enum BuiltinError {
    #[error("expected {expected}, got {got}")]
    Arity { expected: Arity, got: usize },
    #[error("argument {index} expected {expected}, got {got}")]
    Type {
        index: usize,
        /// The type as the dialogue names it, `"a number"` covering both.
        expected: &'static str,
        got: ValueType,
    },
    #[error("argument {index} and argument 0 are not of the same type")]
    Mixed { index: usize },
    #[error("integer overflow")]
    Overflow,
}

pub struct Builtin {
    pub name: &'static str,
    pub arity: Arity,
    pub doc: &'static str,
    call: fn(Vec<Value>) -> Result<Value, BuiltinError>,
}

impl Builtin {
    const fn new(
        name: &'static str,
        arity: Arity,
        doc: &'static str,
        call: fn(Vec<Value>) -> Result<Value, BuiltinError>,
    ) -> Self {
        Self {
            name,
            arity,
            doc,
            call,
        }
    }

    pub fn call(&self, args: Vec<Value>) -> Result<Value, BuiltinError> {
        if !self.arity.accepts(args.len()) {
            return Err(BuiltinError::Arity {
                expected: self.arity,
                got: args.len(),
            });
        }

        (self.call)(args)
    }
}

// ---- Reading the arguments

/// The argument `index`, which the arity has already made sure is there.
fn at(args: &[Value], index: usize) -> &Value {
    &args[index]
}

fn as_string(args: &[Value], index: usize) -> Result<&str, BuiltinError> {
    match at(args, index) {
        Value::String(text) => Ok(text),
        other => Err(BuiltinError::Type {
            index,
            expected: "a string",
            got: other.vtype(),
        }),
    }
}

fn as_float(args: &[Value], index: usize) -> Result<f64, BuiltinError> {
    match at(args, index) {
        Value::Float(number) => Ok(*number),
        other => Err(BuiltinError::Type {
            index,
            expected: "a float",
            got: other.vtype(),
        }),
    }
}

fn as_int(args: &[Value], index: usize) -> Result<i64, BuiltinError> {
    match at(args, index) {
        Value::Int(number) => Ok(*number),
        other => Err(BuiltinError::Type {
            index,
            expected: "an int",
            got: other.vtype(),
        }),
    }
}

// ---- Functions

/// `upper("bell")`
fn upper(args: Vec<Value>) -> Result<Value, BuiltinError> {
    Ok(Value::String(as_string(&args, 0)?.to_uppercase()))
}

/// `lower("Bell")`
fn lower(args: Vec<Value>) -> Result<Value, BuiltinError> {
    Ok(Value::String(as_string(&args, 0)?.to_lowercase()))
}

/// `capitalize("bell")`
fn capitalize(args: Vec<Value>) -> Result<Value, BuiltinError> {
    let val = as_string(&args, 0)?;
    let val = val
        .split(" ")
        .map(|s| {
            if s.is_empty() {
                return s.into();
            }

            let mut letters = s.chars().collect::<Vec<_>>();
            letters[0] = letters[0].to_uppercase().next().unwrap();
            letters.into_iter().collect::<String>()
        })
        .collect::<Vec<String>>()
        .join(" ");

    Ok(Value::String(val))
}

/// `len("été")` and `len($stat)`
fn len(args: Vec<Value>) -> Result<Value, BuiltinError> {
    let length = match at(&args, 0) {
        Value::String(text) => text.chars().count(),
        Value::Dict(attrs) => attrs.len(),
        other => {
            return Err(BuiltinError::Type {
                index: 0,
                expected: "a string or a dict",
                got: other.vtype(),
            });
        }
    };

    Ok(Value::Int(length as i64))
}

/// `has($stat, "hp")`
fn has(args: Vec<Value>) -> Result<Value, BuiltinError> {
    let Value::Dict(attrs) = at(&args, 0) else {
        return Err(BuiltinError::Type {
            index: 0,
            expected: "a dict",
            got: at(&args, 0).vtype(),
        });
    };

    Ok(Value::Bool(attrs.contains_key(as_string(&args, 1)?)))
}

/// `abs(-3)` and `abs(-3.0)`
/// The type is kept.
fn abs(args: Vec<Value>) -> Result<Value, BuiltinError> {
    match at(&args, 0) {
        Value::Int(number) => number
            .checked_abs()
            .map(Value::Int)
            .ok_or(BuiltinError::Overflow),
        Value::Float(number) => Ok(Value::Float(number.abs())),
        other => Err(BuiltinError::Type {
            index: 0,
            expected: "a number",
            got: other.vtype(),
        }),
    }
}

/// `min(1, 2)` and `min($a, $b, $c)`
fn min(args: Vec<Value>) -> Result<Value, BuiltinError> {
    fold(args, Ordering::Min)
}

/// `max(1, 2)` and `max($a, $b, $c)`
fn max(args: Vec<Value>) -> Result<Value, BuiltinError> {
    fold(args, Ordering::Max)
}

enum Ordering {
    Min,
    Max,
}

/// The first argument says which type the call is about, and every other one
/// has to be of it: `min(1, 2.0)` is refused rather than read as either.
fn fold(args: Vec<Value>, which: Ordering) -> Result<Value, BuiltinError> {
    match at(&args, 0) {
        Value::Int(first) => {
            let mut best = *first;
            for index in 1..args.len() {
                let Value::Int(number) = at(&args, index) else {
                    return Err(BuiltinError::Mixed { index });
                };
                best = match which {
                    Ordering::Min => best.min(*number),
                    Ordering::Max => best.max(*number),
                };
            }
            Ok(Value::Int(best))
        }
        Value::Float(first) => {
            let mut best = *first;
            for index in 1..args.len() {
                let Value::Float(number) = at(&args, index) else {
                    return Err(BuiltinError::Mixed { index });
                };
                best = match which {
                    Ordering::Min => best.min(*number),
                    Ordering::Max => best.max(*number),
                };
            }
            Ok(Value::Float(best))
        }
        other => Err(BuiltinError::Type {
            index: 0,
            expected: "a number",
            got: other.vtype(),
        }),
    }
}

/// `round(3.551)` and `round(3.551, 2)`
fn round(args: Vec<Value>) -> Result<Value, BuiltinError> {
    let number = as_float(&args, 0)?;
    let precision = match args.len() {
        1 => 0,
        _ => as_int(&args, 1)?,
    };

    let scale = 10f64.powi(precision.clamp(i32::MIN as i64, i32::MAX as i64) as i32);
    Ok(Value::Float((number * scale).round() / scale))
}

/// `floor(3.7)`
fn floor(args: Vec<Value>) -> Result<Value, BuiltinError> {
    Ok(Value::Float(as_float(&args, 0)?.floor()))
}

/// `ceil(3.2)`
fn ceil(args: Vec<Value>) -> Result<Value, BuiltinError> {
    Ok(Value::Float(as_float(&args, 0)?.ceil()))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn call(name: &str, args: Vec<Value>) -> Result<Value, BuiltinError> {
        lookup(name).expect("a builtin of this name").call(args)
    }

    fn str(text: &str) -> Value {
        Value::String(text.to_owned())
    }

    fn stat() -> Value {
        Value::Dict(HashMap::from([("hp".to_owned(), Value::Int(12))]))
    }

    #[test]
    fn a_name_no_builtin_answers_to_is_not_one() {
        assert!(lookup("upper").is_some());
        assert!(lookup("teleport").is_none());
    }

    /// The length of the call is checked before the builtin runs, so every
    /// `fn` of this module may read the arguments it was promised.
    #[test]
    fn a_call_of_the_wrong_length_never_reaches_the_builtin() {
        assert_eq!(
            call("upper", vec![]),
            Err(BuiltinError::Arity {
                expected: Arity::Exact(1),
                got: 0,
            }),
        );
        assert_eq!(
            call(
                "round",
                vec![Value::Float(1.0), Value::Int(1), Value::Int(1)]
            )
            .unwrap_err()
            .to_string(),
            "expected 1 to 2 arguments, got 3",
        );
        assert_eq!(
            call("min", vec![Value::Int(1)]).unwrap_err().to_string(),
            "expected at least 2 arguments, got 1",
        );
    }

    #[test]
    fn a_string_is_cased_and_counted_in_characters() {
        assert_eq!(call("upper", vec![str("bell")]), Ok(str("BELL")));
        assert_eq!(call("lower", vec![str("Bell")]), Ok(str("bell")));
        assert_eq!(
            call("capitalize", vec![str(" jean  michel ")]),
            Ok(str(" Jean  Michel "))
        );
        assert_eq!(call("len", vec![str("été")]), Ok(Value::Int(3)));
    }

    #[test]
    fn a_dict_is_counted_and_asked_for_a_key() {
        assert_eq!(call("len", vec![stat()]), Ok(Value::Int(1)));
        assert_eq!(call("has", vec![stat(), str("hp")]), Ok(Value::Bool(true)));
        assert_eq!(call("has", vec![stat(), str("mp")]), Ok(Value::Bool(false)));
    }

    /// A number keeps the type it was written with, here as everywhere else.
    #[test]
    fn a_number_keeps_its_type() {
        assert_eq!(call("abs", vec![Value::Int(-3)]), Ok(Value::Int(3)));
        assert_eq!(call("abs", vec![Value::Float(-3.0)]), Ok(Value::Float(3.0)));
        assert_eq!(
            call("min", vec![Value::Int(2), Value::Int(1), Value::Int(3)]),
            Ok(Value::Int(1)),
        );
        assert_eq!(
            call("max", vec![Value::Float(2.0), Value::Float(3.5)]),
            Ok(Value::Float(3.5)),
        );
    }

    /// Two types in one call is a mistake, not a conversion.
    #[test]
    fn two_types_in_one_comparison_are_refused() {
        assert_eq!(
            call("min", vec![Value::Int(1), Value::Float(2.0)]),
            Err(BuiltinError::Mixed { index: 1 }),
        );
    }

    #[test]
    fn rounding_answers_a_float_at_the_precision_it_is_given() {
        assert_eq!(
            call("round", vec![Value::Float(3.5)]),
            Ok(Value::Float(4.0))
        );
        assert_eq!(
            call("round", vec![Value::Float(3.551), Value::Int(2)]),
            Ok(Value::Float(3.55)),
        );
        assert_eq!(
            call("round", vec![Value::Float(1234.0), Value::Int(-2)]),
            Ok(Value::Float(1200.0)),
        );
        assert_eq!(
            call("floor", vec![Value::Float(3.7)]),
            Ok(Value::Float(3.0))
        );
        assert_eq!(call("ceil", vec![Value::Float(3.2)]), Ok(Value::Float(4.0)));
    }

    /// The message names the argument at fault and the type it holds, so the
    /// log points at what to fix in the line.
    #[test]
    fn an_argument_of_the_wrong_type_is_named() {
        assert_eq!(
            call("upper", vec![Value::Int(1)]),
            Err(BuiltinError::Type {
                index: 0,
                expected: "a string",
                got: ValueType::Int,
            }),
        );
        assert_eq!(
            call("has", vec![stat(), Value::Int(1)])
                .unwrap_err()
                .to_string(),
            "argument 1 expected a string, got int",
        );
        assert_eq!(
            call("round", vec![Value::Int(3)]).unwrap_err().to_string(),
            "argument 0 expected a float, got int",
        );
    }
}
