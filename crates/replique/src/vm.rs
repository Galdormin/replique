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

use thiserror::Error;

use crate::dialogue::{Dialogue, DialogueNode, NodeName, Step, StepId, StepKind, Value};

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
}

impl DialogueVm {
    /// Start `dialogue` at the node `name` and run up to the first event.
    ///
    /// Fails with [`VmError::DialogueNotFinished`] if a run is still in
    /// progress, and with [`VmError::DialogueNodeNotFound`] if the node does
    /// not exist. Starting again after [`DialogueEvent::Finished`] is fine and
    /// resets the VM.
    pub fn start(
        &mut self,
        dialogue: Dialogue,
        name: impl Into<NodeName>,
    ) -> Result<DialogueEvent, VmError> {
        if !matches!(self.state, VmState::NotStarted | VmState::Finished) {
            return Err(VmError::DialogueNotFinished);
        }

        self.dialogue = Some(dialogue);

        let cursor = self.get_cursor_for_node(&name.into())?;
        self.run(cursor)
    }

    /// Answer the last [`DialogueEvent`] and run up to the next one.
    ///
    /// `resume` must match the event the VM emitted, see [`ResumeEvent`]. On
    /// error the VM stays suspended where it was, so a wrong or out of range
    /// answer can just be retried.
    pub fn resume(&mut self, resume: ResumeEvent) -> Result<DialogueEvent, VmError> {
        let (mut cursor, next) = match &self.state {
            VmState::Suspended { cursor, at } => (cursor.clone(), at.resolve(&resume)?),
            VmState::NotStarted => return Err(VmError::DialogueNotStarted),
            VmState::Finished => return Err(VmError::DialogueAlreadyFinished),
        };

        cursor.step = next;
        self.run(cursor)
    }

    /// Walk steps from `cursor` until one of them needs the host, saving where
    /// to resume from and returning the matching event.
    fn run(&mut self, mut cursor: Cursor) -> Result<DialogueEvent, VmError> {
        for _ in 0..MAX_STEPS {
            match self.get_step_at(&cursor)?.kind.clone() {
                StepKind::Say { line, next } => {
                    self.state = VmState::Suspended {
                        cursor,
                        at: SuspendedAt::Line { next },
                    };
                    return Ok(DialogueEvent::Say {
                        speaker: line.speaker,
                        text: line.text,
                    });
                }
                StepKind::Command { command, next } => {
                    self.state = VmState::Suspended {
                        cursor,
                        at: SuspendedAt::Command { next },
                    };
                    return Ok(DialogueEvent::Command {
                        name: command.name,
                        args: command.args,
                    });
                }
                StepKind::Choice { choices } => {
                    self.state = VmState::Suspended {
                        cursor,
                        at: SuspendedAt::Choice {
                            targets: choices.iter().map(|c| c.target).collect(),
                        },
                    };
                    return Ok(DialogueEvent::Choices {
                        choices: choices.into_iter().map(|c| c.text).collect(),
                    });
                }
                StepKind::Jump(name) => cursor = self.get_cursor_for_node(&name)?,
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
    fn get_cursor_for_node(&self, name: &NodeName) -> Result<Cursor, VmError> {
        let mut cursor = Cursor {
            name: name.clone(),
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dialogue::builder::DialogueNodeBuilder;
    use crate::dialogue::{ChoiceDef, Command, TextLine};

    fn line(speaker: Option<&str>, text: &str, next: StepId) -> StepKind {
        StepKind::Say {
            line: TextLine {
                speaker: speaker.map(String::from),
                text: text.into(),
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
                args: vec![Value::String("bell".into()), Value::Float(0.5)],
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
                    text: "go left".into(),
                    target: left,
                },
                ChoiceDef {
                    text: "go right".into(),
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
}
