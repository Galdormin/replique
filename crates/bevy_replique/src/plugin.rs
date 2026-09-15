use bevy::{
    app::{Plugin, PostUpdate},
    asset::AssetApp,
    ecs::schedule::{IntoScheduleConfigs, SystemSet},
};

use crate::{
    asset::{RepliqueDialogue, RepliqueDialogueLoader},
    call::command::{DialogueCommandRegistry, run_dialogue_commands},
    call::function::DialogueFunctionRegistry,
    message::{
        DialogueChoices, DialogueCommand, DialogueFinished, DialogueLine, ResumeDialogue,
        StartDialogue,
    },
    runner::{resume_dialogue, start_dialogue, start_pending_dialogue},
};

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum DialogueSystem {
    Runner,
    Commands,
}

pub struct RepliquePlugin;

impl Plugin for RepliquePlugin {
    fn build(&self, app: &mut bevy::app::App) {
        app.configure_sets(
            PostUpdate,
            (DialogueSystem::Runner, DialogueSystem::Commands).chain(),
        );

        app.init_resource::<DialogueCommandRegistry>()
            .init_resource::<DialogueFunctionRegistry>();

        app.init_asset::<RepliqueDialogue>()
            .init_asset_loader::<RepliqueDialogueLoader>();

        app.add_message::<DialogueLine>()
            .add_message::<DialogueCommand>()
            .add_message::<DialogueChoices>()
            .add_message::<DialogueFinished>()
            .add_message::<StartDialogue>()
            .add_message::<ResumeDialogue>();

        app.add_systems(
            PostUpdate,
            (start_dialogue, resume_dialogue, start_pending_dialogue)
                .chain()
                .in_set(DialogueSystem::Runner),
        );

        app.add_systems(
            PostUpdate,
            run_dialogue_commands.in_set(DialogueSystem::Commands),
        );
    }
}
