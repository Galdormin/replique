//! Answering the functions of a dialogue.
//!
//! Where a `>>` command *does* something, a function *answers* something. The
//! dialogue asks the game for a value and carries on with it, without ever
//! suspending. The answer of a function is a [`Value`]
//!
//! For example: `[let $stat = get_stat(Alice)]` reads a component off the entity `Alice`,
//! `[party_size()]` counts them.
//!
//! The arguments arrive already typed, as they do for a command, and the
//! return type is written back by [`IntoValue`](crate::args::IntoValue):
//!
//! ```
//! # use bevy::prelude::*;
//! # use bevy_replique::prelude::*;
//!
//! #[derive(Resource)]
//! struct Gold(i64);
//!
//! /// `Alice: I have [gold()] coins.`
//! fn gold(In(()): In<()>, gold: Res<Gold>) -> i64 {
//!     gold.0
//! }
//!
//! # let mut app = App::new();
//! # app.insert_resource(Gold(12));
//! # app.add_dialogue_function_named("gold", gold);
//! ```
//!
//! **Declare the name over the system.** `#[replique_function]` writes down
//! what the dialogue calls it, next to what it does, and
//! [`add_dialogue_function`](DialogueFunctionAppExt::add_dialogue_function)
//! registers it from there. This is the way to register a function: the name
//! is written once, where it can be read and documented.
//!
//! ```
//! # use bevy::prelude::*;
//! # use bevy_replique::prelude::*;
//! # #[derive(Resource)]
//! # struct Gold(i64);
//! /// Coins the player has, as the dialogue reads it.
//! #[replique_function(name = "gold")]
//! fn coins(In(()): In<()>, gold: Res<Gold>) -> i64 {
//!     gold.0
//! }
//!
//! # let mut app = App::new();
//! # app.insert_resource(Gold(12));
//! app.add_dialogue_function(coins);
//! # assert_eq!(coins::NAME, "gold");
//! ```
//!
//! [`add_dialogue_function_named`](DialogueFunctionAppExt::add_dialogue_function_named)
//! takes the name as a string instead, for a system the attribute cannot be
//! put on.
//!
//! **A function may only read.** The `ReadOnlySystem` bound makes it a compile
//! error to register one that spawns, mutates a component or writes a message.
//! Writing to the world is what a `>>` command is for.
//!
//! **A function cannot suspend.** It answers on the spot, inside the call to
//! [`DialogueVm::resume_with`](replique::vm::DialogueVm::resume_with).
//! Anything that has to wait for the player is a command.
//!
//! A function that has no answer to give says so with a `Result`, which is return
//! by the VM with an [`VmError::ExprEvalError`](replique::vm::VmError::ExprEvalError).
//!
//! ```
//! # use bevy::prelude::*;
//! # use bevy_replique::prelude::*;
//! #[derive(Component, PartialEq)]
//! struct Name(String);
//!
//! /// `[let $stat = hp_of("Alice")]`
//! fn hp_of(In((who,)): In<(String,)>, names: Query<&Name>) -> Result<i64, String> {
//!     names
//!         .iter()
//!         .find(|name| name.0 == who)
//!         .map(|_| 12)
//!         .ok_or_else(|| format!("{who} is not on the scene"))
//! }
//! # let mut app = App::new();
//! # app.add_dialogue_function_named("hp_of", hp_of);
//! ```
//!
//! The `bevy_custom_function` example holds a full scene built this way.

use bevy::{
    app::App,
    ecs::{
        entity::Entity,
        resource::Resource,
        system::{In, IntoSystem, ReadOnlySystem},
        world::{Mut, World},
    },
    log::error,
    platform::collections::HashMap,
};
use replique::{
    builtins::lookup,
    dialogue::Value,
    host::{HostError, RepliqueHost},
};

use crate::args::{DialogueArgs, FromDialogueArgs, IntoFunctionOutput};

type FunctionRunner =
    Box<dyn Fn(&mut World, DialogueArgs) -> Result<Value, HostError> + Send + Sync>;

#[derive(Resource, Default)]
pub(crate) struct DialogueFunctionRegistry {
    functions: HashMap<String, FunctionRunner>,
}

/// A function declared with `#[replique_function]`, name and doc included.
/// ```
/// # use bevy::prelude::*;
/// # use bevy_replique::prelude::*;
/// /// The name in upper case, to shout it.
/// #[replique_function(name = "upper")]
/// fn shout(In((text,)): In<(String,)>) -> String {
///     text.to_uppercase()
/// }
///
/// assert_eq!(shout::NAME, "upper");
/// assert_eq!(shout::DOC, "The name in upper case, to shout it.");
/// ```
///
/// Reach it through [`add_dialogue_function`], which is the whole of what it
/// is for.
///
/// [`add_dialogue_function`]: DialogueFunctionAppExt::add_dialogue_function
pub trait RepliqueFunction {
    /// What the dialogue writes to call it.
    const NAME: &'static str;
    /// The documentation the attribute read off the system.
    const DOC: &'static str;

    fn register(self, app: &mut App);
}

/// Registers the system to answer a function.
///
/// Two ways in, and [`add_dialogue_function`] is the one to reach for: the
/// name of the function then lives with the system it names, where it can be
/// read and documented, instead of being repeated as a string at the point of
/// registration. [`add_dialogue_function_named`] is for a system the attribute
/// cannot be put on — one coming from another crate, or one whose name is only
/// known at runtime.
///
/// Both refuse a name a builtin already answers to: `upper` and `len` are part
/// of the language, and a game that could shadow them would make the same line
/// read differently from one game to the next. The registration is skipped and
/// logged.
///
/// **A function may only read.** The `ReadOnlySystem` bound makes it a compile
/// error to register one that spawns, mutates a component or writes a message:
/// a function runs in the middle of a line being rendered, so an effect of its
/// own would depend on how often that happens. Writing to the world is what a
/// `>>` command is for.
///
/// [`add_dialogue_function`]: DialogueFunctionAppExt::add_dialogue_function
/// [`add_dialogue_function_named`]: DialogueFunctionAppExt::add_dialogue_function_named
pub trait DialogueFunctionAppExt {
    /// Registers a system declared with `#[replique_function]`, under the name
    /// the attribute gave it.
    ///
    /// This is the way to register a function. There is no name to repeat
    /// here, hence none to get wrong: the one the dialogue writes is the one
    /// written over the system.
    ///
    /// ```
    /// # use bevy::prelude::*;
    /// # use bevy_replique::prelude::*;
    /// /// `Bob: [upper("alice")] is looking for you.`
    /// #[replique_function]
    /// fn upper(In((text,)): In<(String,)>) -> String {
    ///     text.to_uppercase()
    /// }
    /// # let mut app = App::new();
    /// app.add_dialogue_function(upper);
    /// ```
    ///
    /// Registering the same name twice keeps the last system. A name the
    /// dialogue writes and nobody registered is an error when the expression
    /// is evaluated.
    fn add_dialogue_function<S>(&mut self, system: S) -> &mut Self
    where
        S: RepliqueFunction;

    /// Registers a system under a name given here, for the cases
    /// [`add_dialogue_function`] cannot cover: a system from a crate you do
    /// not control, or a name only known once the game runs.
    ///
    /// Prefer the attribute when you can. A name written here is a second
    /// place to keep in step with the dialogue, and nothing checks that the
    /// two agree.
    ///
    /// ```
    /// # use bevy::prelude::*;
    /// # use bevy_replique::prelude::*;
    /// /// `Bob: [upper("alice")] is looking for you.`
    /// fn upper(In((text,)): In<(String,)>) -> String {
    ///     text.to_uppercase()
    /// }
    /// # let mut app = App::new();
    /// app.add_dialogue_function_named("upper", upper);
    /// ```
    ///
    /// Registering the same name twice keeps the last system. A name the
    /// dialogue writes and nobody registered is an error when the expression
    /// is evaluated.
    ///
    /// [`add_dialogue_function`]: DialogueFunctionAppExt::add_dialogue_function
    fn add_dialogue_function_named<I, O, OM, M, S>(
        &mut self,
        name: impl Into<String>,
        system: S,
    ) -> &mut Self
    where
        I: FromDialogueArgs + Send + Sync + 'static,
        O: IntoFunctionOutput<OM> + Send + Sync + 'static,
        S: IntoSystem<In<I>, O, M> + 'static,
        S::System: ReadOnlySystem;
}

impl DialogueFunctionAppExt for App {
    fn add_dialogue_function_named<I, O, OM, M, S>(
        &mut self,
        name: impl Into<String>,
        system: S,
    ) -> &mut Self
    where
        I: FromDialogueArgs + Send + Sync + 'static,
        O: IntoFunctionOutput<OM> + Send + Sync + 'static,
        S: IntoSystem<In<I>, O, M> + 'static,
        S::System: ReadOnlySystem,
    {
        let name = name.into();

        if lookup(&name).is_some() {
            error!("A builtin function with the name {name} already exists.");
            return self;
        }

        let world = self.world_mut();
        let id = world.register_system(system);

        let label = name.clone();
        let run: FunctionRunner = Box::new(move |world, args| {
            let input = match I::from_dialogue_args(args) {
                Ok(input) => input,
                Err(err) => {
                    return Err(HostError::Failed {
                        name: label.clone(),
                        message: err.to_string(),
                    });
                }
            };

            let output = world
                .run_system_with(id, input)
                .map_err(|err| HostError::Failed {
                    name: label.clone(),
                    message: err.to_string(),
                })?;

            output
                .into_function_output()
                .map_err(|message| HostError::Failed {
                    name: label.clone(),
                    message,
                })
        });

        world
            .get_resource_or_insert_with(DialogueFunctionRegistry::default)
            .functions
            .insert(name, run);

        self
    }

    fn add_dialogue_function<S>(&mut self, system: S) -> &mut Self
    where
        S: RepliqueFunction,
    {
        system.register(self);
        self
    }
}

/// What the VM asks when an expression names a function.
pub(crate) struct DialogueHost<'a> {
    world: &'a mut World,
    runner: Entity,
}

impl<'a> DialogueHost<'a> {
    pub fn new(world: &'a mut World, runner: Entity) -> Self {
        Self { world, runner }
    }
}

impl<'a> RepliqueHost for DialogueHost<'a> {
    fn call(&mut self, name: &str, args: Vec<Value>) -> Result<Value, HostError> {
        self.world
            .resource_scope(|world, registry: Mut<DialogueFunctionRegistry>| {
                if let Some(func) = registry.functions.get(name) {
                    func(
                        world,
                        DialogueArgs {
                            runner: self.runner,
                            args,
                        },
                    )
                } else {
                    Err(HostError::UnknownFunction(name.into()))
                }
            })
    }
}
