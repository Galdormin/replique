//! Virtual machine executing a compiled [`Dialogue`].
//!
//! The VM is a coroutine, not a loop. [`DialogueVm::start`] walks the steps of
//! a node until it reaches something only the host can handle, then suspends
//! and hands back a [`DialogueEvent`].
//!
//! The host does whatever that event means for it, then calls [`DialogueVm::resume`]
//! with a [`ResumeEvent`] to let the VM carry on.
//!
//! ```rust
//! use replique::RepliqueFile;
//! use replique::vm::{DialogueEvent, DialogueVm, ResumeEvent};
//!
//! let file = RepliqueFile::from_source(
//!     ":= start\nAlice: Hi!\n-> Hello\n    Bob: Hello!\n-> Bye\n    Bob: Bye!\n---\n",
//! );
//! let dialogue = file.dialogue.unwrap();
//!
//! let mut vm = DialogueVm::default();
//! let event = vm.start(dialogue, "start").unwrap();
//! assert!(matches!(event, DialogueEvent::Say { .. }));
//!
//! let DialogueEvent::Choices { choices } = vm.resume(ResumeEvent::Advance).unwrap()
//! else {
//!     panic!("expected a choice");
//! };
//! assert_eq!(choices, ["Hello", "Bye"]);
//!
//! let DialogueEvent::Say { text, .. } = vm.resume(ResumeEvent::Select(1)).unwrap()
//! else {
//!     panic!("expected a line");
//! };
//! assert_eq!(text, "Bye!");
//! ```
//! A [`DialogueVm`] never panics on a malformed dialogue: every way a run can
//! go wrong is a [`VmError`], including the step limit that catches
//! nodes jumping to each other forever.

use std::collections::HashMap;

use thiserror::Error;

use crate::{
    dialogue::{
        Dialogue, DialogueNode, NodeName, Step, StepId, StepKind, TextPart, Value,
        expr::{EvalError, Expr},
    },
    host::{NoHost, RepliqueHost},
};

/// How many steps a single [`DialogueVm::start`] or [`DialogueVm::resume`] may
/// walk before giving up with [`VmError::StepLimitExceeded`].
///
/// Only steps invisible to the host are counted, since any other step suspends
/// the VM and resets the budget. In practice this bounds how many jumps can be
/// chained, and stops a cycle of nodes jumping to each other from hanging the
/// caller.
const MAX_STEPS: usize = 200;

/// What the VM asks the host to do before the dialogue can go on.
///
/// Returned by [`DialogueVm::start`] and [`DialogueVm::resume`]. Every variant
/// but [`Finished`](DialogueEvent::Finished) leaves the VM suspended, waiting
/// for the matching [`ResumeEvent`].
pub enum DialogueEvent {
    /// A line of dialogue to show. Resume with [`ResumeEvent::Advance`] once
    /// the player is done reading it.
    Say {
        /// Who is talking, or `None` for a line written without a speaker.
        speaker: Option<String>,
        text: String,
    },
    /// A command for the host to interpret, such as `>> add_scene(Alice)`.
    /// Meaning and arguments are entirely up to the host. Resume with
    /// [`ResumeEvent::Advance`], immediately or once the command is over.
    Command { name: String, args: Vec<Value> },
    /// A choice to offer to the player. Resume with
    /// [`ResumeEvent::Select`] carrying the index of the chosen entry.
    Choices {
        /// Texts of the choices, in the order they were written.
        choices: Vec<String>,
    },
    /// The dialogue reached its end. The VM is no longer suspended, so
    /// [`DialogueVm::resume`] now fails and only
    /// [`DialogueVm::start`] can be called again.
    Finished,
}

/// How the host answers the [`DialogueEvent`] it was given.
///
/// The answer must match the event:
/// - [`DialogueEvent::Choices`] -> [`Select`](ResumeEvent::Select)
/// - [`DialogueEvent::Say`] -> [`Advance`](ResumeEvent::Advance)
/// - [`DialogueEvent::Command`] -> [`Advance`](ResumeEvent::Advance)
/// - [`DialogueEvent::Finished`] -> Nothing
///
/// A mismatch is a [`VmError::WrongResumeEvent`] and leaves the VM untouched,
/// so the host can simply try again with the right one.
pub enum ResumeEvent {
    /// Advance to the next step.
    Advance,
    /// Take the choice at this index in the [`DialogueEvent::Choices`]
    Select(usize),
}

/// What the [`DialogueVm`] is suspended on, and where each way out leads.
enum SuspendedAt {
    Line { next: StepId },
    Command { next: StepId },
    Choice { targets: Vec<StepId> },
}

impl SuspendedAt {
    /// Step to run next for this answer, or a [`VmError`] if the answer does
    /// not fit what the VM is suspended on.
    fn resolve(&self, resume: &ResumeEvent) -> Result<StepId, VmError> {
        match (self, resume) {
            (Self::Line { next } | Self::Command { next }, ResumeEvent::Advance) => Ok(*next),
            (Self::Choice { targets }, ResumeEvent::Select(i)) => {
                targets.get(*i).copied().ok_or(VmError::ChoiceOutOfRange {
                    index: *i,
                    count: targets.len(),
                })
            }
            _ => Err(VmError::WrongResumeEvent),
        }
    }
}

/// Where the VM currently is: a step, and the node it belongs to.
#[derive(Clone)]
struct Cursor {
    name: NodeName,
    step: StepId,
}

/// Context of evaluation for [`crate::dialogue::expr::Expr`].
pub(crate) struct EvalCtx<'a, H: RepliqueHost> {
    pub vars: &'a VarStore,
    pub host: &'a mut H,
}

// Store of variable
#[derive(Default)]
pub struct VarStore {
    vars: HashMap<String, Value>,
}

impl VarStore {
    pub fn new(vars: HashMap<String, Value>) -> Self {
        Self { vars }
    }

    pub fn vars(&self) -> &HashMap<String, Value> {
        &self.vars
    }

    pub fn insert(&mut self, name: String, val: Value) {
        self.vars.insert(name, val);
    }

    pub fn get(&self, name: &str) -> Option<&Value> {
        self.vars.get(name)
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut Value> {
        self.vars.get_mut(name)
    }
}

/// Everything that can go wrong while running a [`Dialogue`].
///
/// A failed call never changes the state of the VM. After an error it is still
/// suspended exactly where it was, and the host can resume with a valid answer.
#[derive(Error, Debug)]
pub enum VmError {
    /// [`DialogueVm::resume`] was called before [`DialogueVm::start`].
    #[error("Dialogue not started")]
    DialogueNotStarted,
    /// [`DialogueVm::start`] was called while a dialogue is still running.
    /// Starting again is only allowed once the current one is finished.
    #[error("Dialogue not finished")]
    DialogueNotFinished,
    /// [`DialogueVm::resume`] was called after [`DialogueEvent::Finished`].
    #[error("Dialogue already finished")]
    DialogueAlreadyFinished,
    /// A node was asked for by name, by [`DialogueVm::start`] and the
    /// [`Dialogue`] has none.
    #[error("DialogueNode not found with name {0:?}")]
    DialogueNodeNotFound(NodeName),
    /// [`ResumeEvent::Select`] carried an index no choice has.
    #[error("Choice is out of range: {index} > {count}")]
    ChoiceOutOfRange { index: usize, count: usize },
    /// The [`ResumeEvent`] does not fit the [`DialogueEvent`] the VM emitted,
    /// such as an [`Advance`](ResumeEvent::Advance) on a pending choice.
    #[error("Wrong ResumeEvent")]
    WrongResumeEvent,
    /// Too many steps were walked without anything to hand back to the host,
    /// which means the dialogue loops on itself.
    #[error("Step limit exceeded")]
    StepLimitExceeded,
    /// Error when evaluating expression
    #[error("EvalError: {0}")]
    ExprEvalError(#[from] EvalError),
}

/// State of a [`DialogueVm`] between two calls.
#[derive(Default)]
enum VmState {
    #[default]
    NotStarted,
    Suspended {
        cursor: Cursor,
        at: SuspendedAt,
    },
    Finished,
}

/// Runs one [`Dialogue`], one event at a time.
///
/// Create it with [`Default`], then alternate [`start`](DialogueVm::start) and
/// [`resume`](DialogueVm::resume) as described in the [module
/// documentation](self). The same VM can be reused for another run, or another
/// node, once the current dialogue is finished.
///
/// A `DialogueVm` holds the [`Dialogue`] at start but can be changed each time a
/// [`start`](DialogueVm::start) is called.
#[derive(Default)]
pub struct DialogueVm {
    dialogue: Option<Dialogue>,
    state: VmState,
    vars: VarStore,
}

impl DialogueVm {
    /// Same as [`DialogueVm::start_with`] but with [`NoHost`] as host.
    pub fn start(
        &mut self,
        dialogue: Dialogue,
        name: impl Into<NodeName>,
    ) -> Result<DialogueEvent, VmError> {
        self.start_with(&mut NoHost, dialogue, name)
    }

    /// Start `dialogue` at the node `name` and run up to the first event.
    ///
    /// The caller give a [`RepliqueHost`] to give access to its registered functions.
    ///
    /// Fails with [`VmError::DialogueNotFinished`] if a run is still in
    /// progress, and with [`VmError::DialogueNodeNotFound`] if the node does
    /// not exist. Starting again after [`DialogueEvent::Finished`] is fine and
    /// resets the VM.
    pub fn start_with<H: RepliqueHost>(
        &mut self,
        host: &mut H,
        dialogue: Dialogue,
        name: impl Into<NodeName>,
    ) -> Result<DialogueEvent, VmError> {
        if !matches!(self.state, VmState::NotStarted | VmState::Finished) {
            return Err(VmError::DialogueNotFinished);
        }

        self.dialogue = Some(dialogue);

        let cursor = self.get_cursor_for_node(name.into())?;
        self.run(host, cursor)
    }

    /// Every variable the dialogue has written, and its value.
    /// Use for tests
    #[allow(unused)]
    pub fn vars(&self) -> &VarStore {
        &self.vars
    }

    /// Same as [`DialogueVm::resume_with`] but with [`NoHost`] as host.
    pub fn resume(&mut self, resume: ResumeEvent) -> Result<DialogueEvent, VmError> {
        self.resume_with(&mut NoHost, resume)
    }

    /// Answer the last [`DialogueEvent`] and run up to the next one.
    ///
    /// The caller give a [`RepliqueHost`] to give access to its registered functions.
    ///
    /// `resume` must match the event the VM emitted, see [`ResumeEvent`]. On
    /// error the VM stays suspended where it was, so a wrong or out of range
    /// answer can just be retried.
    pub fn resume_with<H: RepliqueHost>(
        &mut self,
        host: &mut H,
        resume: ResumeEvent,
    ) -> Result<DialogueEvent, VmError> {
        let (mut cursor, next) = match &self.state {
            VmState::Suspended { cursor, at } => (cursor.clone(), at.resolve(&resume)?),
            VmState::NotStarted => return Err(VmError::DialogueNotStarted),
            VmState::Finished => return Err(VmError::DialogueAlreadyFinished),
        };

        cursor.step = next;
        self.run(host, cursor)
    }

    /// Walk steps from `cursor` until one of them needs the host, saving where
    /// to resume from and returning the matching event.
    fn run<H: RepliqueHost>(
        &mut self,
        host: &mut H,
        mut cursor: Cursor,
    ) -> Result<DialogueEvent, VmError> {
        for _ in 0..MAX_STEPS {
            match self.get_step_at(&cursor)?.kind.clone() {
                StepKind::Say { line, next } => {
                    // Rendered before the state moves: an inline expression
                    // that does not evaluate must leave the VM where it was.
                    let text = self.render_text(host, line.text)?;

                    self.state = VmState::Suspended {
                        cursor,
                        at: SuspendedAt::Line { next },
                    };
                    return Ok(DialogueEvent::Say {
                        speaker: line.speaker,
                        text,
                    });
                }
                StepKind::Command { command, next } => {
                    // Evaluated before the state moves: an argument that does
                    // not evaluate must leave the VM where it was.
                    let args = command
                        .args
                        .into_iter()
                        .map(|e| self.eval(host, e))
                        .collect::<Result<_, _>>()?;

                    self.state = VmState::Suspended {
                        cursor,
                        at: SuspendedAt::Command { next },
                    };
                    return Ok(DialogueEvent::Command {
                        name: command.name,
                        args,
                    });
                }
                StepKind::Set {
                    name,
                    attrs,
                    value,
                    next,
                } => {
                    let value = self.eval(host, value)?;
                    if attrs.is_empty() {
                        self.vars.insert(name, value);
                    } else {
                        self.set_attr(&name, &attrs, value)?;
                    }
                    cursor.step = next;
                }
                StepKind::Branch {
                    condition,
                    then,
                    otherwise,
                } => {
                    cursor.step = match self.eval(host, condition)? {
                        Value::Bool(true) => then,
                        Value::Bool(false) => otherwise,
                        other => {
                            return Err(EvalError::NotACondition(other.vtype().to_string()).into());
                        }
                    };
                }
                StepKind::Choice { choices } => {
                    let targets = choices.iter().map(|c| c.target).collect();
                    let texts = choices
                        .into_iter()
                        .map(|c| self.render_text(host, c.text))
                        .collect::<Result<_, _>>()?;

                    self.state = VmState::Suspended {
                        cursor,
                        at: SuspendedAt::Choice { targets },
                    };
                    return Ok(DialogueEvent::Choices { choices: texts });
                }
                StepKind::Jump(name) => cursor = self.get_cursor_for_node(name)?,
                StepKind::End => {
                    self.state = VmState::Finished;
                    return Ok(DialogueEvent::Finished);
                }
            }
        }
        Err(VmError::StepLimitExceeded)
    }

    /// Return a reference to the current dialogue
    fn dialogue(&self) -> Result<&Dialogue, VmError> {
        self.dialogue.as_ref().ok_or(VmError::DialogueNotStarted)
    }

    /// Cursor on the entry step of `name`.
    fn get_cursor_for_node(&self, name: NodeName) -> Result<Cursor, VmError> {
        let mut cursor = Cursor {
            name,
            step: StepId::default(),
        };

        let node = self.get_node_at(&cursor)?;
        cursor.step = node.entry;
        Ok(cursor)
    }

    /// Node the cursor is in.
    fn get_node_at(&self, cursor: &Cursor) -> Result<&DialogueNode, VmError> {
        self.dialogue()?
            .get_node(&cursor.name)
            .ok_or(VmError::DialogueNodeNotFound(cursor.name.clone()))
    }

    /// Step the cursor points at.
    fn get_step_at(&self, cursor: &Cursor) -> Result<&Step, VmError> {
        Ok(self.get_node_at(cursor)?.get_step(&cursor.step))
    }

    /// Eval an Expr against the variables of the VM and the functions `host`
    /// answers for.
    ///
    /// The [`EvalCtx`] is built here and dies with the call, so the borrow it
    /// takes on the variables never outlives one expression.
    fn eval<H: RepliqueHost>(&self, host: &mut H, expr: Expr) -> Result<Value, EvalError> {
        expr.eval(&mut EvalCtx {
            vars: &self.vars,
            host,
        })
    }

    /// The text a line or a choice reads as, with every inline expression
    /// replaced by what it evaluates to.
    fn render_text<H: RepliqueHost>(
        &self,
        host: &mut H,
        parts: Vec<TextPart>,
    ) -> Result<String, EvalError> {
        let mut out = String::new();

        for part in parts {
            match part {
                TextPart::Text(text) => out.push_str(&text),
                TextPart::Expression(expr) => {
                    let val = self.eval(host, expr)?;
                    out.push_str(&val.to_string())
                }
            }
        }

        Ok(out)
    }

    /// Walks `attrs` down the dicts held by the variable `name`, and writes
    /// `value` where the path ends.
    ///
    /// Nothing is created along the way: a variable that does not exist, or a
    /// key a dict does not hold, is an error rather than a new entry, so that
    /// a mistyped name is reported instead of quietly making a field up.
    fn set_attr(&mut self, name: &str, attrs: &[String], value: Value) -> Result<(), EvalError> {
        let mut target = self
            .vars
            .get_mut(name)
            .ok_or_else(|| EvalError::UnknownVariable(name.to_owned()))?;

        for attr in attrs {
            target = match target {
                Value::Dict(map) => map
                    .get_mut(attr)
                    .ok_or_else(|| EvalError::DictHasNoAttr(attr.clone()))?,
                other => {
                    return Err(EvalError::AttrExpectedDict {
                        name: attr.clone(),
                        received: other.vtype().to_string(),
                    });
                }
            };
        }

        *target = value;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dialogue::builder::DialogueNodeBuilder;
    use crate::dialogue::{ChoiceDef, Command, TextLine, expr::Expr};
    use crate::host::{HostError, test::TestHost};
    use crate::parser::expr::BinaryOp;

    fn lit(value: i64) -> Expr {
        Expr::Litteral(Value::Int(value))
    }

    fn line(speaker: Option<&str>, text: &str, next: StepId) -> StepKind {
        StepKind::Say {
            line: TextLine {
                speaker: speaker.map(String::from),
                text: vec![TextPart::Text(text.into())],
            },
            next,
        }
    }

    fn linear_dialogue() -> Dialogue {
        let mut b = DialogueNodeBuilder::default();
        let end = b.push(StepKind::End);
        let l1 = b.push(line(None, "second", end));
        let l0 = b.push(line(Some("Alice"), "first", l1));
        Dialogue::new(vec![b.build(NodeName::new("start"), l0).unwrap()])
    }

    fn command_dialogue() -> Dialogue {
        let mut b = DialogueNodeBuilder::default();
        let end = b.push(StepKind::End);
        let l1 = b.push(line(None, "after", end));
        let cmd = b.push(StepKind::Command {
            command: Command {
                name: "play".into(),
                args: vec![
                    Expr::Litteral(Value::String("bell".into())),
                    Expr::Litteral(Value::Float(0.5)),
                ],
            },
            next: l1,
        });
        let l0 = b.push(line(None, "before", cmd));
        Dialogue::new(vec![b.build(NodeName::new("start"), l0).unwrap()])
    }

    fn choice_dialogue() -> Dialogue {
        let mut b = DialogueNodeBuilder::default();
        let end = b.push(StepKind::End);
        let left = b.push(line(None, "left", end));
        let right = b.push(line(None, "right", end));
        let c = b.push(StepKind::Choice {
            choices: vec![
                ChoiceDef {
                    text: vec![TextPart::Text("go left".into())],
                    target: left,
                },
                ChoiceDef {
                    text: vec![TextPart::Text("go right".into())],
                    target: right,
                },
            ],
        });
        Dialogue::new(vec![b.build(NodeName::new("start"), c).unwrap()])
    }

    fn jump_dialogue(target: &str) -> Dialogue {
        let mut b = DialogueNodeBuilder::default();
        let jump = b.push(StepKind::Jump(NodeName::new(target)));
        let l0 = b.push(line(None, "before jump", jump));
        let start = b.build(NodeName::new("start"), l0).unwrap();

        let mut b = DialogueNodeBuilder::default();
        let end = b.push(StepKind::End);
        let l0 = b.push(line(None, "after jump", end));
        let second = b.build(NodeName::new("second"), l0).unwrap();

        Dialogue::new(vec![start, second])
    }

    fn expect_line(event: DialogueEvent) -> (Option<String>, String) {
        match event {
            DialogueEvent::Say { speaker, text } => (speaker, text),
            _ => panic!("expected a Line event"),
        }
    }

    fn expect_choices(event: DialogueEvent) -> Vec<String> {
        match event {
            DialogueEvent::Choices { choices } => choices,
            _ => panic!("expected a Choices event"),
        }
    }

    fn expect_err(result: Result<DialogueEvent, VmError>) -> VmError {
        match result {
            Err(err) => err,
            Ok(_) => panic!("expected an error"),
        }
    }

    fn expect_finished(event: DialogueEvent) {
        assert!(
            matches!(event, DialogueEvent::Finished),
            "expected a Finished event"
        );
    }

    #[test]
    fn start_emits_the_entry_line() {
        let mut vm = DialogueVm::default();

        let (speaker, text) = expect_line(vm.start(linear_dialogue(), "start").unwrap());

        assert_eq!(speaker.as_deref(), Some("Alice"));
        assert_eq!(text, "first");
    }

    #[test]
    fn advance_walks_the_lines() {
        let mut vm = DialogueVm::default();

        vm.start(linear_dialogue(), "start").unwrap();
        let (speaker, text) = expect_line(vm.resume(ResumeEvent::Advance).unwrap());
        assert_eq!(speaker, None);
        assert_eq!(text, "second");

        expect_finished(vm.resume(ResumeEvent::Advance).unwrap());
    }

    #[test]
    fn error_on_start_unknown_node() {
        let mut vm = DialogueVm::default();

        let err = expect_err(vm.start(linear_dialogue(), "nope"));

        assert!(matches!(err, VmError::DialogueNodeNotFound(n) if n == NodeName::new("nope")));
    }

    #[test]
    fn error_on_resume_before_start() {
        let mut vm = DialogueVm::default();

        let err = expect_err(vm.resume(ResumeEvent::Advance));

        assert!(matches!(err, VmError::DialogueNotStarted));
    }

    #[test]
    fn error_on_resume_after_the_end() {
        let mut vm = DialogueVm::default();

        vm.start(linear_dialogue(), "start").unwrap();
        vm.resume(ResumeEvent::Advance).unwrap();
        expect_finished(vm.resume(ResumeEvent::Advance).unwrap());

        let err = expect_err(vm.resume(ResumeEvent::Advance));
        assert!(matches!(err, VmError::DialogueAlreadyFinished));
    }

    #[test]
    fn error_on_start_while_suspended() {
        let mut vm = DialogueVm::default();

        vm.start(linear_dialogue(), "start").unwrap();
        let err = expect_err(vm.start(linear_dialogue(), "start"));

        assert!(matches!(err, VmError::DialogueNotFinished));
    }

    #[test]
    fn start_again_after_the_end_is_allowed() {
        let mut vm = DialogueVm::default();

        vm.start(linear_dialogue(), "start").unwrap();
        vm.resume(ResumeEvent::Advance).unwrap();
        expect_finished(vm.resume(ResumeEvent::Advance).unwrap());

        let (_, text) = expect_line(vm.start(linear_dialogue(), "start").unwrap());
        assert_eq!(text, "first");
    }

    #[test]
    fn a_command_suspends_the_dialogue_with_its_name_and_arguments() {
        let mut vm = DialogueVm::default();

        vm.start(command_dialogue(), "start").unwrap();
        let event = vm.resume(ResumeEvent::Advance).unwrap();

        let DialogueEvent::Command { name, args } = event else {
            panic!("expected a Command event");
        };
        assert_eq!(name, "play");
        assert_eq!(args, vec![Value::String("bell".into()), Value::Float(0.5)]);
    }

    #[test]
    fn advance_carries_on_after_a_command() {
        let mut vm = DialogueVm::default();

        vm.start(command_dialogue(), "start").unwrap();
        vm.resume(ResumeEvent::Advance).unwrap();
        let (_, text) = expect_line(vm.resume(ResumeEvent::Advance).unwrap());

        assert_eq!(text, "after");
    }

    #[test]
    fn error_on_select_on_a_command() {
        let mut vm = DialogueVm::default();

        vm.start(command_dialogue(), "start").unwrap();
        vm.resume(ResumeEvent::Advance).unwrap();
        let err = expect_err(vm.resume(ResumeEvent::Select(0)));

        assert!(matches!(err, VmError::WrongResumeEvent));
    }

    #[test]
    fn choice_emits_every_choice_text_in_order() {
        let mut vm = DialogueVm::default();

        let choices = expect_choices(vm.start(choice_dialogue(), "start").unwrap());

        assert_eq!(choices, vec!["go left".to_string(), "go right".to_string()]);
    }

    #[test]
    fn select_follows_the_matching_target() {
        for (index, expected) in [(0, "left"), (1, "right")] {
            let mut vm = DialogueVm::default();

            vm.start(choice_dialogue(), "start").unwrap();
            let (_, text) = expect_line(vm.resume(ResumeEvent::Select(index)).unwrap());

            assert_eq!(text, expected);
        }
    }

    #[test]
    fn error_on_select_out_of_range_and_keeps_the_choice_pending() {
        let mut vm = DialogueVm::default();

        vm.start(choice_dialogue(), "start").unwrap();
        let err = expect_err(vm.resume(ResumeEvent::Select(5)));
        assert!(matches!(
            err,
            VmError::ChoiceOutOfRange { index: 5, count: 2 }
        ));

        // The VM is still suspended on the same choice, so a valid retry works.
        let (_, text) = expect_line(vm.resume(ResumeEvent::Select(0)).unwrap());
        assert_eq!(text, "left");
    }

    #[test]
    fn error_on_advance_on_a_choice() {
        let mut vm = DialogueVm::default();

        vm.start(choice_dialogue(), "start").unwrap();
        let err = expect_err(vm.resume(ResumeEvent::Advance));

        assert!(matches!(err, VmError::WrongResumeEvent));
    }

    #[test]
    fn error_on_select_on_a_line() {
        let mut vm = DialogueVm::default();

        vm.start(linear_dialogue(), "start").unwrap();
        let err = expect_err(vm.resume(ResumeEvent::Select(0)));

        assert!(matches!(err, VmError::WrongResumeEvent));
    }

    /// A node that assigns `$gold`, then hands it to a command, so the value
    /// the host receives is the one the assignment computed.
    fn set_dialogue(value: Expr) -> Dialogue {
        let mut b = DialogueNodeBuilder::default();
        let end = b.push(StepKind::End);
        let cmd = b.push(StepKind::Command {
            command: Command {
                name: "show".into(),
                args: vec![Expr::Var("gold".into())],
            },
            next: end,
        });
        let set = b.push(StepKind::Set {
            name: "gold".into(),
            attrs: vec![],
            value,
            next: cmd,
        });
        Dialogue::new(vec![b.build(NodeName::new("start"), set).unwrap()])
    }

    fn expect_command(event: DialogueEvent) -> (String, Vec<Value>) {
        match event {
            DialogueEvent::Command { name, args } => (name, args),
            _ => panic!("expected a Command event"),
        }
    }

    #[test]
    fn an_assignment_is_invisible_and_gives_a_variable_its_value() {
        let mut vm = DialogueVm::default();

        let (name, args) = expect_command(vm.start(set_dialogue(lit(10)), "start").unwrap());

        assert_eq!(name, "show");
        assert_eq!(args, [Value::Int(10)]);
    }

    #[test]
    fn an_assignment_reads_the_variables_written_before_it() {
        let mut b = DialogueNodeBuilder::default();
        let end = b.push(StepKind::End);
        let cmd = b.push(StepKind::Command {
            command: Command {
                name: "show".into(),
                args: vec![Expr::Var("gold".into())],
            },
            next: end,
        });
        let second = b.push(StepKind::Set {
            name: "gold".into(),
            attrs: vec![],
            value: Expr::Binary {
                op: BinaryOp::Add,
                lhs: Box::new(Expr::Var("gold".into())),
                rhs: Box::new(lit(5)),
            },
            next: cmd,
        });
        let first = b.push(StepKind::Set {
            name: "gold".into(),
            attrs: vec![],
            value: lit(10),
            next: second,
        });
        let dialogue = Dialogue::new(vec![b.build(NodeName::new("start"), first).unwrap()]);
        let mut vm = DialogueVm::default();

        let (_, args) = expect_command(vm.start(dialogue, "start").unwrap());

        assert_eq!(args, [Value::Int(15)]);
    }

    /// `[let $player = {...}]` then `[let $player.<attr> = <value>]`, as the
    /// compiler lays it out: two `Set` steps in a row.
    fn set_attr_dialogue(attrs: &[&str], value: Expr) -> Dialogue {
        let mut b = DialogueNodeBuilder::default();
        let end = b.push(StepKind::End);
        let write = b.push(StepKind::Set {
            name: "player".into(),
            attrs: attrs.iter().map(|a| (*a).to_string()).collect(),
            value,
            next: end,
        });
        let init = b.push(StepKind::Set {
            name: "player".into(),
            attrs: vec![],
            value: Expr::LitteralDict(HashMap::from([(
                "stats".to_owned(),
                Expr::LitteralDict(HashMap::from([("hp".to_owned(), lit(10))])),
            )])),
            next: write,
        });
        Dialogue::new(vec![b.build(NodeName::new("start"), init).unwrap()])
    }

    fn dict(entries: &[(&str, Value)]) -> Value {
        Value::Dict(
            entries
                .iter()
                .map(|(key, val)| ((*key).to_owned(), val.clone()))
                .collect(),
        )
    }

    #[test]
    fn an_assignment_writes_through_a_path_of_attributes() {
        let mut vm = DialogueVm::default();

        vm.start(set_attr_dialogue(&["stats", "hp"], lit(3)), "start")
            .unwrap();

        assert_eq!(
            vm.vars().get("player"),
            Some(&dict(&[("stats", dict(&[("hp", Value::Int(3))]))]))
        );
    }

    /// The attr names what to replace, so writing over a dict is allowed.
    #[test]
    fn an_assignment_can_replace_a_whole_dict() {
        let mut vm = DialogueVm::default();

        vm.start(set_attr_dialogue(&["stats"], lit(0)), "start")
            .unwrap();

        assert_eq!(
            vm.vars().get("player"),
            Some(&dict(&[("stats", Value::Int(0))]))
        );
    }

    /// Nothing is created along the way: a key the dict does not hold is a
    /// mistyped name, not a new entry.
    #[test]
    fn error_on_an_attribute_the_dict_does_not_hold() {
        let mut vm = DialogueVm::default();

        let err = expect_err(vm.start(set_attr_dialogue(&["stats", "mp"], lit(3)), "start"));

        assert!(matches!(
            err,
            VmError::ExprEvalError(EvalError::DictHasNoAttr(name)) if name == "mp"
        ));
    }

    #[test]
    fn error_on_an_attribute_read_on_something_that_is_not_a_dict() {
        let mut vm = DialogueVm::default();

        let err = expect_err(vm.start(
            set_attr_dialogue(&["stats", "hp", "deeper"], lit(3)),
            "start",
        ));

        assert!(matches!(
            err,
            VmError::ExprEvalError(EvalError::AttrExpectedDict { name, .. }) if name == "deeper"
        ));
    }

    #[test]
    fn error_on_an_attribute_written_on_a_variable_that_does_not_exist() {
        let mut b = DialogueNodeBuilder::default();
        let end = b.push(StepKind::End);
        let write = b.push(StepKind::Set {
            name: "unknown".into(),
            attrs: vec!["hp".to_owned()],
            value: lit(3),
            next: end,
        });
        let dialogue = Dialogue::new(vec![b.build(NodeName::new("start"), write).unwrap()]);
        let mut vm = DialogueVm::default();

        let err = expect_err(vm.start(dialogue, "start"));

        assert!(matches!(
            err,
            VmError::ExprEvalError(EvalError::UnknownVariable(name)) if name == "unknown"
        ));
    }

    /// A variable nobody wrote is an error, not a default value.
    #[test]
    fn error_on_a_variable_that_was_never_assigned() {
        let mut vm = DialogueVm::default();

        let err = expect_err(vm.start(set_dialogue(Expr::Var("unknown".into())), "start"));

        assert!(matches!(
            err,
            VmError::ExprEvalError(EvalError::UnknownVariable(name)) if name == "unknown"
        ));
    }

    /// `[if <condition>] Alice: oui [else] Alice: non`, as the compiler
    /// lays it out: one branch step, two bodies, both rejoining the end.
    fn branch_dialogue(condition: Expr) -> Dialogue {
        let mut b = DialogueNodeBuilder::default();
        let end = b.push(StepKind::End);
        let yes = b.push(line(None, "oui", end));
        let no = b.push(line(None, "non", end));
        let branch = b.push(StepKind::Branch {
            condition,
            then: yes,
            otherwise: no,
        });
        Dialogue::new(vec![b.build(NodeName::new("start"), branch).unwrap()])
    }

    #[test]
    fn a_branch_takes_the_body_its_condition_points_at() {
        for (condition, expected) in [(true, "oui"), (false, "non")] {
            let mut vm = DialogueVm::default();
            let dialogue = branch_dialogue(Expr::Litteral(Value::Bool(condition)));

            let (_, text) = expect_line(vm.start(dialogue, "start").unwrap());

            assert_eq!(text, expected, "condition {condition}");
        }
    }

    #[test]
    fn a_branch_reads_the_variables_written_before_it() {
        let mut b = DialogueNodeBuilder::default();
        let end = b.push(StepKind::End);
        let yes = b.push(line(None, "riche", end));
        let branch = b.push(StepKind::Branch {
            condition: Expr::Binary {
                op: BinaryOp::Gt,
                lhs: Box::new(Expr::Var("gold".into())),
                rhs: Box::new(lit(5)),
            },
            then: yes,
            otherwise: end,
        });
        let set = b.push(StepKind::Set {
            name: "gold".into(),
            attrs: vec![],
            value: lit(10),
            next: branch,
        });
        let dialogue = Dialogue::new(vec![b.build(NodeName::new("start"), set).unwrap()]);
        let mut vm = DialogueVm::default();

        let (_, text) = expect_line(vm.start(dialogue, "start").unwrap());

        assert_eq!(text, "riche");
    }

    /// Only a condition the parser could not type can get here, since a
    /// variable has no type before the dialogue runs.
    #[test]
    fn error_on_a_condition_that_is_not_a_bool_at_run_time() {
        let mut vm = DialogueVm::default();

        let err = expect_err(vm.start(branch_dialogue(lit(1)), "start"));

        assert!(matches!(
            err,
            VmError::ExprEvalError(EvalError::NotACondition(vtype)) if vtype == "int"
        ));
    }

    /// The whole chain, from the source to the line the host sees.
    #[test]
    fn a_written_condition_runs() {
        let src = ":= start\n\
             [let $gold = 10]\n\
             [if $gold > 5]\n\
             \x20   Alice: J'ai plus de 5 pièces\n\
             [else]\n\
             \x20   Alice: J'ai pas d'argent\n\
             ---\n";
        let file = crate::RepliqueFile::from_source(src);
        assert_eq!(
            file.diagnostics.errors(),
            0,
            "{}",
            file.diagnostics
                .render(src, crate::parser::diagnostic::Color::Never)
        );

        let mut vm = DialogueVm::default();
        let (_, text) = expect_line(vm.start(file.dialogue.unwrap(), "start").unwrap());

        assert_eq!(text, "J'ai plus de 5 pièces");
    }

    #[test]
    fn jump_continues_in_the_target_node() {
        let mut vm = DialogueVm::default();

        vm.start(jump_dialogue("second"), "start").unwrap();
        let (_, text) = expect_line(vm.resume(ResumeEvent::Advance).unwrap());
        assert_eq!(text, "after jump");

        expect_finished(vm.resume(ResumeEvent::Advance).unwrap());
    }

    #[test]
    fn error_on_jump_to_unknown_node() {
        let mut vm = DialogueVm::default();

        vm.start(jump_dialogue("missing"), "start").unwrap();
        let err = expect_err(vm.resume(ResumeEvent::Advance));

        assert!(matches!(err, VmError::DialogueNodeNotFound(n) if n == NodeName::new("missing")));
    }

    #[test]
    fn error_on_self_jumping_node() {
        let mut b = DialogueNodeBuilder::default();
        let jump = b.push(StepKind::Jump(NodeName::new("loop")));
        let dialogue = Dialogue::new(vec![b.build(NodeName::new("loop"), jump).unwrap()]);
        let mut vm = DialogueVm::default();

        let err = expect_err(vm.start(dialogue, "loop"));

        assert!(matches!(err, VmError::StepLimitExceeded));
    }

    /// A `Say` whose text is `prefix` followed by what `expr` evaluates to.
    fn host_line(prefix: &str, expr: Expr, next: StepId) -> StepKind {
        StepKind::Say {
            line: TextLine {
                speaker: None,
                text: vec![TextPart::Text(prefix.into()), TextPart::Expression(expr)],
            },
            next,
        }
    }

    fn call(name: &str, args: Vec<Expr>) -> Expr {
        Expr::Function {
            name: name.to_owned(),
            args,
        }
    }

    #[test]
    fn an_inline_expression_asks_the_host_for_its_functions() {
        let mut b = DialogueNodeBuilder::default();
        let end = b.push(StepKind::End);
        let l0 = b.push(host_line(
            "Bonjour ",
            call("upper", vec![Expr::Litteral(Value::String("alice".into()))]),
            end,
        ));
        let dialogue = Dialogue::new(vec![b.build(NodeName::new("start"), l0).unwrap()]);
        let mut vm = DialogueVm::default();
        let mut host = TestHost::default();

        let event = vm.start_with(&mut host, dialogue, "start").unwrap();

        assert_eq!(expect_line(event).1, "Bonjour ALICE");
        assert_eq!(host.called(), ["upper"]);
    }

    /// The host is only reached when a step names a function: a dialogue that
    /// calls none never touches it.
    #[test]
    fn a_dialogue_without_a_function_never_calls_the_host() {
        let mut vm = DialogueVm::default();
        let mut host = TestHost::default();

        vm.start_with(&mut host, linear_dialogue(), "start")
            .unwrap();
        vm.resume_with(&mut host, ResumeEvent::Advance).unwrap();

        assert_eq!(host.called(), [] as [&str; 0]);
    }

    #[test]
    fn an_assignment_can_hold_what_the_host_answered() {
        let mut vm = DialogueVm::default();
        let mut host = TestHost::default();
        let dialogue = set_dialogue(call("double", vec![lit(21)]));

        let event = vm.start_with(&mut host, dialogue, "start").unwrap();

        assert_eq!(expect_command(event).1, [Value::Int(42)]);
        assert_eq!(vm.vars().get("gold"), Some(&Value::Int(42)));
    }

    /// A function sees the variables as they are when it is called, not as
    /// they were written: the assignment above it has already run.
    #[test]
    fn a_function_is_given_the_variables_the_dialogue_wrote() {
        let mut b = DialogueNodeBuilder::default();
        let end = b.push(StepKind::End);
        let say = b.push(host_line(
            "",
            call("double", vec![Expr::Var("gold".into())]),
            end,
        ));
        let set = b.push(StepKind::Set {
            name: "gold".into(),
            attrs: vec![],
            value: lit(4),
            next: say,
        });
        let dialogue = Dialogue::new(vec![b.build(NodeName::new("start"), set).unwrap()]);
        let mut vm = DialogueVm::default();
        let mut host = TestHost::default();

        let event = vm.start_with(&mut host, dialogue, "start").unwrap();

        assert_eq!(expect_line(event).1, "8");
        assert_eq!(host.calls, [("double".to_owned(), vec![Value::Int(4)])]);
    }

    #[test]
    fn a_branch_can_turn_on_what_the_host_answers() {
        for (n, expected) in [(4, "oui"), (3, "non")] {
            let mut vm = DialogueVm::default();
            let mut host = TestHost::default();
            let dialogue = branch_dialogue(call("is_even", vec![lit(n)]));

            let event = vm.start_with(&mut host, dialogue, "start").unwrap();

            assert_eq!(expect_line(event).1, expected, "is_even({n})");
        }
    }

    #[test]
    fn a_command_argument_can_come_from_the_host() {
        let mut b = DialogueNodeBuilder::default();
        let end = b.push(StepKind::End);
        let cmd = b.push(StepKind::Command {
            command: Command {
                name: "show".into(),
                args: vec![call("double", vec![lit(3)]), lit(1)],
            },
            next: end,
        });
        let dialogue = Dialogue::new(vec![b.build(NodeName::new("start"), cmd).unwrap()]);
        let mut vm = DialogueVm::default();
        let mut host = TestHost::default();

        let (name, args) = expect_command(vm.start_with(&mut host, dialogue, "start").unwrap());

        assert_eq!(name, "show");
        assert_eq!(args, [Value::Int(6), Value::Int(1)]);
    }

    /// A call the host refuses is an error like any other: the VM stays
    /// suspended on the step before it, so the same answer can be given again
    /// to a host that does know the function.
    #[test]
    fn a_host_that_refuses_a_call_leaves_the_vm_where_it_was() {
        let mut b = DialogueNodeBuilder::default();
        let end = b.push(StepKind::End);
        let l1 = b.push(host_line(
            "",
            call("upper", vec![Expr::Litteral(Value::String("ok".into()))]),
            end,
        ));
        let l0 = b.push(line(None, "before", l1));
        let dialogue = Dialogue::new(vec![b.build(NodeName::new("start"), l0).unwrap()]);
        let mut vm = DialogueVm::default();

        assert_eq!(
            expect_line(vm.start(dialogue, "start").unwrap()).1,
            "before"
        );

        // `NoHost` answers no function at all.
        let err = expect_err(vm.resume(ResumeEvent::Advance));
        assert!(matches!(
            err,
            VmError::ExprEvalError(EvalError::HostError(HostError::UnknownFunction(name)))
                if name == "upper"
        ));

        let mut host = TestHost::default();
        let event = vm.resume_with(&mut host, ResumeEvent::Advance).unwrap();
        assert_eq!(expect_line(event).1, "OK");
    }

    #[test]
    fn error_on_a_function_the_host_fails_to_run() {
        let mut b = DialogueNodeBuilder::default();
        let end = b.push(StepKind::End);
        let l0 = b.push(host_line("", call("boom", vec![]), end));
        let dialogue = Dialogue::new(vec![b.build(NodeName::new("start"), l0).unwrap()]);
        let mut vm = DialogueVm::default();
        let mut host = TestHost::default();

        let err = expect_err(vm.start_with(&mut host, dialogue, "start"));

        assert!(matches!(
            err,
            VmError::ExprEvalError(EvalError::HostError(HostError::Failed { ref name, .. }))
                if name == "boom"
        ));
    }
}
