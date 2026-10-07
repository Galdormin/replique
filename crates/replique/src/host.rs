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
    #[error("subject {0} is unknown")]
    UnknownSubject(String),
    #[error("feature {0} is unknown")]
    UnknownFeature(String),
}

pub trait RepliqueHost {
    /// Call a function through the host. Function are instantaneous
    fn call(&mut self, name: &str, args: Vec<Value>) -> Result<Value, HostError>;

    /// Check if a subject has the given feature
    fn has_feature(&mut self, subject: &str, name: &str) -> Result<bool, HostError>;
}

/// Default host, no additional function available.
/// Builtins are still available.
pub struct NoHost;

impl RepliqueHost for NoHost {
    fn call(&mut self, name: &str, _: Vec<Value>) -> Result<Value, HostError> {
        Err(HostError::UnknownFunction(name.into()))
    }

    fn has_feature(&mut self, _: &str, _: &str) -> Result<bool, HostError> {
        Ok(false)
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

        fn has_feature(&mut self, subject: &str, name: &str) -> Result<bool, HostError> {
            let supported_features = ["happy", "sad", "angry", "hungry", "thirsty"];
            let alice_features = ["happy", "hungry"];
            let bob_features = ["sad", "hungry", "thirsty"];

            if !supported_features.contains(&name) {
                return Err(HostError::UnknownFeature(name.to_string()));
            }

            match subject {
                "Alice" => Ok(alice_features.contains(&name)),
                "Bob" => Ok(bob_features.contains(&name)),
                _ => Err(HostError::UnknownSubject(subject.to_string())),
            }
        }
    }
}
