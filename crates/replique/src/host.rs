//! Define the [`RepliqueHost`] trait for the Virtual Machine to execute
//! function on the host side without sending events.

use thiserror::Error;

use crate::dialogue::Value;

#[derive(Error, Debug)]
pub enum HostError {
    #[error("unknown function `{0}`")]
    UnknownFunction(String),
    #[error("`{name}`: `{message}`")]
    Failed { name: String, message: String },
    #[error("builtin `{name}`: `{message}`")]
    BuiltinFailed { name: String, message: String },
}

pub trait RepliqueHost {
    fn call(&mut self, name: &str, args: Vec<Value>) -> Result<Value, HostError>;
}

/// Default host, no additional function available.
/// Builtins are still available.
pub struct NoHost;

impl RepliqueHost for NoHost {
    fn call(&mut self, name: &str, _: Vec<Value>) -> Result<Value, HostError> {
        Err(HostError::UnknownFunction(name.into()))
    }
}

#[cfg(test)]
pub(crate) mod test {
    use super::*;

    /// A host with a handful of functions, which writes down every call it is
    /// given.
    ///
    /// The functions are the smallest set the tests need: one over a string,
    /// two over an int, and one that always fails.
    #[derive(Default)]
    pub(crate) struct TestHost {
        /// Every call, in the order the VM made them, arguments already
        /// evaluated.
        pub calls: Vec<(String, Vec<Value>)>,
    }

    impl TestHost {
        /// Names of the functions called so far, for a test that only cares
        /// about which ones ran.
        pub fn called(&self) -> Vec<&str> {
            self.calls.iter().map(|(name, _)| name.as_str()).collect()
        }
    }

    impl RepliqueHost for TestHost {
        fn call(&mut self, name: &str, args: Vec<Value>) -> Result<Value, HostError> {
            self.calls.push((name.to_owned(), args.clone()));

            let failed = |message: &str| HostError::Failed {
                name: name.to_owned(),
                message: message.to_owned(),
            };

            match name {
                "upper_new" => match args.as_slice() {
                    [Value::String(text)] => Ok(Value::String(text.to_uppercase())),
                    _ => Err(failed("expected one string")),
                },
                "double" => match args.as_slice() {
                    [Value::Int(n)] => Ok(Value::Int(n * 2)),
                    _ => Err(failed("expected one int")),
                },
                "is_even" => match args.as_slice() {
                    [Value::Int(n)] => Ok(Value::Bool(n % 2 == 0)),
                    _ => Err(failed("expected one int")),
                },
                "boom" => Err(failed("this function always fails")),
                _ => Err(HostError::UnknownFunction(name.to_owned())),
            }
        }
    }
}
