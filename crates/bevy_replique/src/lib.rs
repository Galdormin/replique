pub mod asset;
pub mod call;
pub mod message;
pub mod plugin;
pub mod runner;

pub mod prelude {
    pub use bevy_replique_derive::{
        RepliqueArgs, RepliqueValue, replique_command, replique_function,
    };
    pub use replique::dialogue::{NodeName, Value, ValueType};

    pub use crate::asset::*;
    pub use crate::call::args::*;
    pub use crate::call::command::*;
    pub use crate::call::function::*;
    pub use crate::call::*;
    pub use crate::message::*;
    pub use crate::plugin::*;
    pub use crate::runner::*;
}
