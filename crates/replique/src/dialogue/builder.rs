//! Builds a [`DialogueNode`] and checks it holds together.
//!
//! Steps are pushed one by one, each push handing back the [`StepId`] the next
//! ones can point at, and [`DialogueNodeBuilder::build`] closes the node once
//! every step is in. Building is where the guarantees the
//! [`vm`](crate::vm) relies on are established: every id points at a step that
//! exists, and no step points at itself. That is what lets the VM index
//! [`steps`](DialogueNode) directly instead of checking each move.
//!
//! A [`BuildError`] means the code producing the steps is wrong, not the `.rep`
//! file. Mistakes a writer can make are caught by the parser and reported as
//! diagnostics long before a node is built.

use thiserror::Error;

use crate::dialogue::{DialogueNode, NodeName, Step, StepId, StepKind};

/// Why a node could not be closed.
#[derive(Error, Debug)]
pub(crate) enum BuildError {
    /// A step points at an id no step has.
    #[error("Dangling target found from {from:?} to {to:?}")]
    DanglingTarget { from: StepId, to: StepId },
    /// The entry id given to [`DialogueNodeBuilder::build`] is not one of the
    /// steps that were pushed.
    #[error("Entry is dangling")]
    DanglingEntry,
    /// No step was pushed, so the node has nothing to start on.
    #[error("Empty node")]
    EmptyNode,
    /// A step points at itself, which would spin forever at run time.
    #[error("Self referencing for {0:?}")]
    SelfReferencing(StepId),
}

/// Collects the steps of one node until it can be closed.
///
/// Ids are handed out as steps are pushed, so a step can only point at one
/// pushed before it. Building a node backwards, the way the
/// [`compiler`](crate::dialogue::compiler) does, makes that fall into place on
/// its own: a step is always pushed after the step it leads to.
#[derive(Default)]
pub(crate) struct DialogueNodeBuilder {
    steps: Vec<StepKind>,
}

impl DialogueNodeBuilder {
    /// Add a step and return the id it can now be reached by.
    pub fn push(&mut self, kind: StepKind) -> StepId {
        self.steps.push(kind);
        StepId((self.steps.len() - 1) as u32)
    }

    /// Close the node, starting on `entry`.
    ///
    /// Every step is checked before the node is handed over, so a
    /// [`DialogueNode`] that exists is one the VM can walk without ever
    /// checking an id again. Jump targets are the exception: they name another
    /// node and can only be resolved against the whole
    /// [`Dialogue`](crate::dialogue::Dialogue), so an unknown one surfaces at
    /// run time as a
    /// [`VmError::DialogueNodeNotFound`](crate::vm::VmError::DialogueNodeNotFound).
    pub fn build(self, name: NodeName, entry: StepId) -> Result<DialogueNode, BuildError> {
        if self.steps.is_empty() {
            return Err(BuildError::EmptyNode);
        }

        let max_id = (self.steps.len() - 1) as u32;

        if entry.id() > max_id {
            return Err(BuildError::DanglingEntry);
        }

        let steps = self
            .steps
            .into_iter()
            .enumerate()
            .map(|(id, kind)| {
                let id = id as u32;

                // Self referencing target
                match &kind {
                    StepKind::Say { next, .. } => {
                        if next.id() == id {
                            return Err(BuildError::SelfReferencing(StepId(id)));
                        }
                    }
                    StepKind::Command { next, .. } => {
                        if next.id() == id {
                            return Err(BuildError::SelfReferencing(StepId(id)));
                        }
                    }
                    StepKind::Choice { choices }
                        if choices.iter().find(|c| c.target.id() == id).is_some() =>
                    {
                        return Err(BuildError::SelfReferencing(StepId(id)));
                    }
                    _ => (),
                }

                // Dangling target
                match has_dangling_target(&kind, max_id) {
                    Some(t) => Err(BuildError::DanglingTarget {
                        from: StepId(id),
                        to: t,
                    }),
                    None => Ok(Step { kind }),
                }
            })
            .collect::<Result<Vec<_>, BuildError>>()?;

        Ok(DialogueNode { name, entry, steps })
    }
}

/// First target of this step that no step answers to, if any.
///
/// [`StepKind::Jump`] has no target here: it leads to another node by name, not
/// to a step of this one.
fn has_dangling_target(kind: &StepKind, max_id: u32) -> Option<StepId> {
    let targets = match kind {
        StepKind::Say { next, .. } => vec![next],
        StepKind::Command { next, .. } => vec![next],
        StepKind::Choice { choices } => choices.iter().map(|c| &c.target).collect(),
        StepKind::Jump(_) | StepKind::End => vec![],
    };
    targets.into_iter().find(|t| t.id() > max_id).copied()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dialogue::{ChoiceDef, TextLine};

    fn line(next: StepId) -> StepKind {
        StepKind::Say {
            line: TextLine {
                speaker: None,
                text: "...".into(),
            },
            next,
        }
    }

    fn choice(targets: &[StepId]) -> StepKind {
        StepKind::Choice {
            choices: targets
                .iter()
                .map(|t| ChoiceDef {
                    text: "...".into(),
                    target: *t,
                })
                .collect(),
        }
    }

    #[test]
    fn push_returns_sequential_ids() {
        let mut b = DialogueNodeBuilder::default();

        assert_eq!(b.push(StepKind::End).id(), 0);
        assert_eq!(b.push(StepKind::End).id(), 1);
        assert_eq!(b.push(StepKind::End).id(), 2);
    }

    #[test]
    fn build_keeps_steps_in_push_order() {
        let mut b = DialogueNodeBuilder::default();
        let end = b.push(StepKind::End);
        let l0 = b.push(line(end));

        let node = b.build(NodeName::new("start"), l0).unwrap();

        assert_eq!(node.name, NodeName::new("start"));
        assert_eq!(node.entry, l0);
        assert_eq!(node.steps.len(), 2);
        assert!(matches!(node.get_step(&end).kind, StepKind::End));
        assert!(matches!(node.get_step(&l0).kind, StepKind::Say { .. }));
    }

    #[test]
    fn error_on_self_referencing_line() {
        let mut b = DialogueNodeBuilder::default();
        let l0 = b.push(line(StepId(0)));

        assert!(matches!(
            b.build(NodeName::new("start"), l0),
            Err(BuildError::SelfReferencing(_))
        ));
    }

    #[test]
    fn error_on_dangling_line_target() {
        let mut b = DialogueNodeBuilder::default();
        let l0 = b.push(line(StepId(42)));

        let Err(BuildError::DanglingTarget { from, to }) = b.build(NodeName::new("start"), l0)
        else {
            panic!("expected a DanglingTarget error");
        };
        assert_eq!(from, l0);
        assert_eq!(to, StepId(42));
    }

    #[test]
    fn error_on_dangling_choice_target() {
        let mut b = DialogueNodeBuilder::default();
        let end = b.push(StepKind::End);
        let c = b.push(choice(&[end, StepId(7)]));

        let Err(BuildError::DanglingTarget { from, to }) = b.build(NodeName::new("start"), c)
        else {
            panic!("expected a DanglingTarget error");
        };
        assert_eq!(from, c);
        assert_eq!(to, StepId(7));
    }
}
