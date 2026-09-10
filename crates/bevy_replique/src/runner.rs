use bevy::{
    asset::{Assets, Handle},
    ecs::{
        component::Component,
        entity::Entity,
        message::{MessageReader, MessageWriter},
        system::{Query, Res, SystemParam},
    },
    log::error,
    prelude::Result,
};
use replique::{
    dialogue::NodeName,
    vm::{DialogueEvent, DialogueVm},
};

use crate::{
    asset::RepliqueDialogue,
    message::{
        DialogueChoice, DialogueChoices, DialogueCommand, DialogueFinished, DialogueLine,
        ResumeDialogue, StartDialogue,
    },
};

#[derive(Component)]
pub struct DialogueRunner {
    dialogue: Handle<RepliqueDialogue>,
    vm: DialogueVm,
    pending_start: Option<NodeName>,
}

impl DialogueRunner {
    pub fn new(dialogue: Handle<RepliqueDialogue>) -> Self {
        Self {
            dialogue,
            vm: DialogueVm::default(),
            pending_start: None,
        }
    }
}

#[derive(SystemParam)]
pub struct DialogueOut<'w> {
    lines: MessageWriter<'w, DialogueLine>,
    commands: MessageWriter<'w, DialogueCommand>,
    choices: MessageWriter<'w, DialogueChoices>,
    finished: MessageWriter<'w, DialogueFinished>,
}

impl DialogueOut<'_> {
    fn emit(&mut self, runner: Entity, ev: DialogueEvent) {
        match ev {
            DialogueEvent::Say { speaker, text } => {
                self.lines.write(DialogueLine {
                    runner,
                    speaker,
                    text,
                });
            }
            DialogueEvent::Command { name, args } => {
                self.commands.write(DialogueCommand { runner, name, args });
            }
            DialogueEvent::Choices { choices } => {
                self.choices.write(DialogueChoices {
                    runner,
                    choices: choices
                        .into_iter()
                        .enumerate()
                        .map(|(index, text)| DialogueChoice { index, text })
                        .collect(),
                });
            }
            DialogueEvent::Finished => {
                self.finished.write(DialogueFinished { runner });
            }
        }
    }
}

pub(super) fn start_dialogue(
    mut start_events: MessageReader<StartDialogue>,
    dialogues: Res<Assets<RepliqueDialogue>>,
    mut runners: Query<&mut DialogueRunner>,
    mut dialogue_out: DialogueOut,
) -> Result<()> {
    for start in start_events.read() {
        let Ok(mut runner) = runners.get_mut(start.runner) else {
            error!("Try to start an unknown DialogueRunner");
            continue;
        };

        let Some(dialogue) = dialogues.get(runner.dialogue.id()) else {
            runner.pending_start = Some(start.node.clone().into());
            continue;
        };

        match runner.vm.start(dialogue.dialogue().clone(), &start.node) {
            Ok(event) => {
                dialogue_out.emit(start.runner, event);
            }
            Err(err) => error!("{}", err),
        }
    }

    Ok(())
}

pub(super) fn start_pending_dialogue(
    dialogues: Res<Assets<RepliqueDialogue>>,
    runners: Query<(Entity, &mut DialogueRunner)>,
    mut dialogue_out: DialogueOut,
) -> Result<()> {
    for (entity, mut runner) in runners {
        if runner.pending_start.is_none() {
            continue;
        };

        let Some(dialogue) = dialogues.get(runner.dialogue.id()) else {
            continue;
        };

        let node = runner.pending_start.take().unwrap(); // Cannot fail
        match runner.vm.start(dialogue.dialogue().clone(), &node) {
            Ok(event) => {
                dialogue_out.emit(entity, event);
            }
            Err(err) => error!("{}", err),
        }
    }

    Ok(())
}

pub(super) fn resume_dialogue(
    mut resume_events: MessageReader<ResumeDialogue>,
    dialogues: Res<Assets<RepliqueDialogue>>,
    mut runners: Query<&mut DialogueRunner>,
    mut dialogue_out: DialogueOut,
) -> Result<()> {
    for resume in resume_events.read() {
        let Ok(mut runner) = runners.get_mut(resume.runner) else {
            error!("Try to resume an unknown DialogueRunner");
            continue;
        };

        if dialogues.get(runner.dialogue.id()).is_none() {
            error!("Try to resume a not loaded RepliqueDialogue");
            continue;
        }

        match runner.vm.resume(resume.input.to_resume_event()) {
            Ok(event) => {
                dialogue_out.emit(resume.runner, event);
            }
            Err(err) => error!("{}", err),
        }
    }

    Ok(())
}
