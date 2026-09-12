//! Turns a parsed file into a runnable [`Dialogue`].
//!
//! The [`ast`](crate::parser::ast) is a tree: statements in order, choices
//! holding their body. A [`Dialogue`] is a flat list of steps linked by id.
//! Compiling is what flattens one into the other.
//!
//! The trick is to build each node **backwards**. A step needs the id of the
//! one that follows it, and an id only exists once the step is pushed, so
//! `build_block` walks the statements in reverse: it starts from what comes
//! after the block and threads that id back through the block, each step being
//! pushed with the id of its successor already in hand. This is why the steps
//! of a node sit in the vector in reverse writing order, and why nothing ever
//! has to be patched up afterwards.
//!
//! Choices fall out of the same walk. The body of a choice is compiled as a
//! block ending where the choice itself ends, so every branch rejoins the rest
//! of the node on its own, without a merge step.
//!
//! Compiling assumes a source the parser accepted. Anything a writer can get
//! wrong is caught before this point, and reported as a
//! [`Diagnostic`](crate::parser::diagnostic::Diagnostic).

use crate::{
    dialogue::{
        ChoiceDef, Command, Dialogue, DialogueNode, StepId, StepKind, TextLine,
        builder::{BuildError, DialogueNodeBuilder},
    },
    parser::{
        END_NODE_NAME, Parsed,
        ast::{NodeDecl, Stmt, StmtKind},
        diagnostic::{Diagnostics, Severity},
    },
};

/// Outcome of a compilation.
/// There is no `Err`: a broken file have no dialogue.
pub struct Compiled {
    /// The runnable dialogue, or `None` if the source had errors.
    pub dialogue: Option<Dialogue>,
    /// Everything found in the source, errors and warnings alike. Handed over
    /// from the parser, whether or not a dialogue came out.
    pub diagnostics: Diagnostics,
}

/// Compile a parsed file.
///
/// A single [`Severity::Error`] is enough to give up: the diagnostics are
/// passed along untouched and no dialogue is produced. Warnings do not stop
/// anything.
pub fn compile(parsed: Parsed) -> Compiled {
    if parsed
        .diagnostics
        .iter()
        .find(|d| d.severity() == Severity::Error)
        .is_some()
    {
        return Compiled {
            dialogue: None,
            diagnostics: parsed.diagnostics,
        };
    }

    let nodes = parsed
        .nodes
        .into_iter()
        .map(compile_node)
        .collect::<Vec<_>>();

    Compiled {
        dialogue: Some(Dialogue::new(nodes)),
        diagnostics: parsed.diagnostics,
    }
}

/// Compile one node declaration.
///
/// The [`StepKind::End`] is pushed first so the whole body can be threaded back
/// from it: everything that falls through the node ends there.
fn compile_node(node: NodeDecl) -> DialogueNode {
    let mut builder = DialogueNodeBuilder::default();

    let end = builder.push(StepKind::End);

    // BuildError here are bugs and should panic
    let start = build_block(&mut builder, node.body, end).unwrap();
    builder.build(node.name.into(), start).unwrap()
}

/// Push the steps of `body` and return the id of its first one.
///
/// `last_id` is where the block goes once it runs out of statements — the rest
/// of the enclosing node, or its end. Statements are walked in reverse so that
/// `current_id`, the successor of the statement being pushed, is always already
/// known. See the [module documentation](self).
///
/// An empty body pushes nothing and gives `last_id` straight back, which is how
/// a choice with no body simply carries on.
fn build_block(
    builder: &mut DialogueNodeBuilder,
    body: Vec<Stmt>,
    last_id: StepId,
) -> Result<StepId, BuildError> {
    let mut current_id = last_id;
    for stmt in body.into_iter().rev() {
        current_id = match stmt.kind {
            StmtKind::Say { speaker, text } => builder.push(StepKind::Say {
                line: TextLine {
                    speaker: speaker.map(|s| s.into_inner()),
                    text: text.into_inner(),
                },
                next: current_id,
            }),
            StmtKind::Command { name, args } => builder.push(StepKind::Command {
                command: Command {
                    name: name.into_inner(),
                    args: args
                        .into_iter()
                        .map(|v| v.value.try_into())
                        .collect::<Result<_, _>>()?,
                },
                next: current_id,
            }),
            StmtKind::Set { name, attrs, value } => builder.push(StepKind::Set {
                name: name.into_inner(),
                attrs: attrs.into_iter().map(|s| s.value).collect(),
                value: value.value.try_into().expect("checked by the parser"),
                next: current_id,
            }),
            StmtKind::If {
                branches,
                otherwise,
            } => {
                let mut next_branch = match otherwise {
                    Some(body) => build_block(builder, body, current_id)?,
                    None => current_id,
                };

                for branch in branches.into_iter().rev() {
                    let then = build_block(builder, branch.body, current_id)?;
                    next_branch = builder.push(StepKind::Branch {
                        condition: branch
                            .condition
                            .value
                            .try_into()
                            .expect("checked by the parser"),
                        then,
                        otherwise: next_branch,
                    });
                }

                next_branch
            }
            // `=> END` is the one jump with no node behind it.
            StmtKind::Jump(node_name) if node_name.value == END_NODE_NAME => {
                builder.push(StepKind::End)
            }
            StmtKind::Jump(node_name) => builder.push(StepKind::Jump(node_name.into())),
            StmtKind::Choice { choices } => {
                let choices = choices
                    .into_iter()
                    .map(|c| {
                        build_block(builder, c.body, current_id).map(|target| ChoiceDef {
                            text: c.text.into_inner(),
                            target,
                        })
                    })
                    .collect::<Result<_, _>>()?;
                builder.push(StepKind::Choice { choices })
            }
        };
    }

    Ok(current_id)
}
