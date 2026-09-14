//! Runnable form of a `.rep` file.
//!
//! Where the [`ast`] mirrors how the source is written —
//! nested blocks, spans pointing back at the text — a [`Dialogue`] is shaped
//! for execution. Each node is a flat list of `Step`, every step naming the
//! id of the one that follows it, so running a dialogue is a walk from step to
//! step with no recursion and no lookup by position in a tree. Nesting in the
//! source, such as the body of a choice, becomes a target id like any other.
//!
//! Spans are gone: everything worth reporting to the writer has already been
//! reported by the parser and the [`compiler`]. What is left is what a game
//! needs at run time.
//!
//! A [`Dialogue`] is built by [`compiler::compile`] from a parsed file, or node
//! by node with a `DialogueNodeBuilder`, and then run by a
//! [`DialogueVm`](crate::vm::DialogueVm).

use std::{collections::HashMap, fmt};

use crate::{
    dialogue::{builder::BuildError, expr::Expr},
    parser::{Spanned, ast},
};

pub(crate) mod builder;
pub mod compiler;
pub(crate) mod expr;

/// All the [`DialogueNode`] of a `.rep` file, looked up by name.
///
/// This is the unit a [`DialogueVm`](crate::vm::DialogueVm) runs: jumps travel
/// between the nodes of one `Dialogue`, and cannot leave it.
#[derive(Debug, Clone)]
pub struct Dialogue {
    nodes: HashMap<NodeName, DialogueNode>,
}

impl Dialogue {
    /// Collect nodes into a dialogue, keyed by their name.
    ///
    /// Duplicate names are not an error here: the last node wins. The parser
    /// already rejects a file that declares the same node twice.
    pub fn new(nodes: Vec<DialogueNode>) -> Self {
        let nodes = nodes.into_iter().map(|n| (n.name.clone(), n)).collect();
        Self { nodes }
    }

    /// Node called `name`, or `None` if this dialogue has none.
    pub fn get_node(&self, name: &NodeName) -> Option<&DialogueNode> {
        self.nodes.get(name)
    }
}

/// Id of a step, used to go from step to step inside a [`DialogueNode`].
///
/// An id is an index into the [`steps`](DialogueNode::steps) of *one* node, so
/// it only means something alongside the node it came from. Moving between
/// nodes goes through [`StepKind::Jump`] and a [`NodeName`] instead.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub(crate) struct StepId(pub(crate) u32);

impl StepId {
    /// The raw index, for bound checks and error messages.
    pub fn id(&self) -> u32 {
        self.0
    }
}

/// Name of a [`DialogueNode`], as written after `:=`.
///
/// Used as the key of a [`Dialogue`] and as the target of a jump. A name is
/// compared verbatim, so `start` and `Start` are two different nodes.
#[derive(PartialEq, Eq, Hash, Clone, Debug)]
pub struct NodeName(pub String);

impl NodeName {
    /// Build a name from anything that can become a [`String`], without
    /// copying an owned one.
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }
}

impl std::fmt::Display for NodeName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Drop the span the parser attached to the name, keeping the [`String`] as
/// is.
impl From<Spanned<String>> for NodeName {
    fn from(value: Spanned<String>) -> Self {
        Self(value.value)
    }
}

/// Clone a borrowed name, so `&NodeName` is accepted anywhere an
/// `impl Into<NodeName>` is asked for.
impl From<&NodeName> for NodeName {
    fn from(value: &NodeName) -> Self {
        value.clone()
    }
}

/// Build a name from anything string-like: `&str`, `String`, `&String`,
/// `Cow<str>`, `Box<str>`...
///
/// Always copies. Use [`NodeName::new`] to hand over a `String` you own.
impl<T: AsRef<str>> From<T> for NodeName {
    fn from(value: T) -> Self {
        Self(value.as_ref().to_owned())
    }
}

/// One node of a dialogue: a name and the steps it is made of.
///
/// The steps are stored in no particular order — the compiler emits them
/// backwards — so reading the node in the order the writer wrote it means
/// following its entry and the `next` of each step, not iterating the vector.
#[derive(Debug, Clone)]
pub struct DialogueNode {
    /// Name the node is declared and jumped to under.
    pub name: NodeName,
    /// Step the node starts on.
    pub(crate) entry: StepId,
    /// Every step of the node, indexed by [`StepId`].
    pub(crate) steps: Vec<Step>,
}

impl DialogueNode {
    /// Step with this id.
    ///
    /// Panics on an id from another node, or on one this node does not have.
    /// A node built by [`builder::DialogueNodeBuilder`] only holds ids of its
    /// own steps, which is what makes this indexing safe.
    pub(crate) fn get_step(&self, id: &StepId) -> &Step {
        &self.steps[id.0 as usize]
    }
}

/// One step of a [`DialogueNode`].
///
/// A wrapper around [`StepKind`], kept as its own type so per-step data such as
/// conditions or tags can be added later without touching every match.
#[derive(Debug, Clone)]
pub(crate) struct Step {
    pub kind: StepKind,
}

/// What a [`Step`] does, and where it goes next.
///
/// Every variant but [`End`](StepKind::End) leads somewhere: to another step of
/// the same node through a [`StepId`], or to another node through a
/// [`NodeName`].
#[derive(Debug, Clone)]
pub(crate) enum StepKind {
    /// A line to show, then `next`.
    Say { line: TextLine, next: StepId },
    /// A branch. Each choice carries the step its body starts on, so the bodies
    /// are plain steps of the node like any other.
    Choice { choices: Vec<ChoiceDef> },
    /// Continue in the entry step of another node. The target is only resolved
    /// when the jump is taken.
    Jump(NodeName),
    /// A command for the host, then `next`.
    Command { command: Command, next: StepId },
    /// Give a variable its value, then `next`.
    Set {
        name: String,
        attrs: Vec<String>,
        value: Expr,
        next: StepId,
    },
    /// One branch of an `[if]`: `then` when the condition holds, `otherwise`
    /// the next branch, the `[else]` or `[elif]`, or what follows the whole block.
    Branch {
        condition: Expr,
        then: StepId,
        otherwise: StepId,
    },
    /// The dialogue is over.
    End,
}

/// A line of dialogue, with who says it.
#[derive(Debug, Clone)]
pub(crate) struct TextLine {
    /// Speaker written before the `:`, or `None` for a line without one.
    pub speaker: Option<String>,
    pub text: Vec<TextPart>,
}

/// Part of a [`TextLine`]
#[derive(Debug, Clone)]
pub(crate) enum TextPart {
    Text(String),
    Expression(Expr),
}

impl TryFrom<ast::TextPart> for TextPart {
    type Error = BuildError;

    fn try_from(value: ast::TextPart) -> Result<Self, BuildError> {
        match value {
            ast::TextPart::Text(s) => Ok(TextPart::Text(s)),
            ast::TextPart::Expression(expr) => Ok(TextPart::Expression(expr.try_into()?)),
        }
    }
}

/// One entry of a [`StepKind::Choice`]: what the player reads, and where
/// picking it leads.
#[derive(Debug, Clone)]
pub(crate) struct ChoiceDef {
    /// Text written after the `->`, as the parts it is made of.
    pub text: Vec<TextPart>,
    /// First step of the body of the choice.
    pub target: StepId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueType {
    Bool,
    Int,
    Float,
    String,
    Dict,
}

impl ValueType {
    pub fn type_of(value: &Value) -> Self {
        match value {
            Value::Bool(_) => Self::Bool,
            Value::Int(_) => Self::Int,
            Value::Float(_) => Self::Float,
            Value::String(_) => Self::String,
            Value::Dict(_) => Self::Dict,
        }
    }
}

impl ValueType {
    /// Whether values of this type take part in arithmetic and comparisons.
    pub fn is_number(&self) -> bool {
        matches!(self, Self::Int | Self::Float)
    }
}

impl fmt::Display for ValueType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let type_str = match self {
            ValueType::Bool => "bool",
            ValueType::Int => "int",
            ValueType::Float => "float",
            ValueType::String => "string",
            ValueType::Dict => "dict",
        };

        f.write_str(type_str)
    }
}

/// An argument of a command.
///
/// The same shape as [`ast::Value`], minus the span.
/// Which type an argument gets is decided by the parser from how it is written,
/// never from what the host expects.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Dict(HashMap<String, Value>),
}

/// Drop the parsing details, keeping the value as it was read.
impl From<ast::Value> for Value {
    fn from(value: ast::Value) -> Self {
        match value {
            ast::Value::Bool(val) => Value::Bool(val),
            ast::Value::String(val) => Value::String(val),
            ast::Value::Float(val) => Value::Float(val),
            ast::Value::Int(val) => Value::Int(val),
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Bool(val) => write!(f, "{val}"),
            Value::Int(val) => write!(f, "{val}"),
            Value::Float(val) => write!(f, "{val}"),
            Value::String(val) => f.write_str(val),
            Value::Dict(hash_map) => {
                // Sorted, because a `HashMap` gives its entries in no set
                // order and this ends up in a line the player reads.
                let mut attrs = hash_map
                    .iter()
                    .map(|(k, v)| format!("{k}: {v}"))
                    .collect::<Vec<_>>();
                attrs.sort();
                write!(f, "{{{}}}", attrs.join(", "))
            }
        }
    }
}

impl Value {
    /// Return the [`ValueType`] of the [`Value`]
    pub fn vtype(&self) -> ValueType {
        ValueType::type_of(self)
    }
}

/// A `>> name(args...)` call, handed to the host untouched.
///
/// Nothing here is checked: an unknown name, or a wrong number of arguments,
/// is only noticed by whoever handles the
/// [`DialogueEvent::Command`](crate::vm::DialogueEvent::Command).
#[derive(Debug, Clone)]
pub(crate) struct Command {
    pub name: String,
    pub args: Vec<Expr>,
}
