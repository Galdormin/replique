//! Minimal UI running `assets/dialogue/simple.rep`.
//!
//! Space advances a line, the number keys pick a choice.
//!
//! ```sh
//! cargo run -p bevy_replique --example bevy_simple
//! ```

use bevy::prelude::*;
use bevy_replique::prelude::*;

/// Node the dialogue starts on.
const START_NODE: &str = "start";

/// Keys used to select a choice, in order.
const CHOICE_KEYS: [KeyCode; 4] = [
    KeyCode::Digit1,
    KeyCode::Digit2,
    KeyCode::Digit3,
    KeyCode::Digit4,
];

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(AssetPlugin {
            // The assets live at the root of the workspace, not in the crate.
            file_path: "../../assets".to_string(),
            ..default()
        }))
        .add_plugins(RepliquePLugin)
        .init_resource::<Waiting>()
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (show_line, show_choices, show_finished, handle_input),
        )
        .run();
}

/// What the runner is currently waiting for, and what input is accepted.
#[derive(Resource, Default)]
enum Waiting {
    /// Nothing to answer: the dialogue is loading, running or finished.
    #[default]
    Nothing,
    /// A [`DialogueLine`] is displayed, waiting for [`ResumeInput::Advance`].
    Line(Entity),
    /// A [`DialogueChoices`] is displayed, waiting for [`ResumeInput::Select`].
    Choices { runner: Entity, count: usize },
}

/// Text node showing the current line.
#[derive(Component)]
struct LineText;

/// Text node showing the current choices, or the hint to advance.
#[derive(Component)]
struct ChoicesText;

fn setup(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut start: MessageWriter<StartDialogue>,
) {
    commands.spawn(Camera2d);

    let dialogue: Handle<RepliqueDialogue> = asset_server.load("dialogue/simple.rep");
    let runner = commands.spawn(DialogueRunner::new(dialogue)).id();

    // The asset is still loading here: the runner keeps the start pending until
    // it is available.
    start.write(StartDialogue {
        runner,
        node: START_NODE.into(),
    });

    commands.spawn((
        Node {
            width: percent(100),
            height: percent(100),
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::End,
            row_gap: px(12),
            padding: UiRect::all(px(24)),
            ..default()
        },
        children![
            (Text::new("..."), TextFont::from_font_size(28.0), LineText,),
            (
                Text::new(""),
                TextFont::from_font_size(20.0),
                TextColor(Color::srgb(0.7, 0.7, 0.8)),
                ChoicesText,
            ),
        ],
    ));
}

fn show_line(
    mut lines: MessageReader<DialogueLine>,
    mut waiting: ResMut<Waiting>,
    mut line_text: Single<&mut Text, With<LineText>>,
    mut choices_text: Single<&mut Text, (With<ChoicesText>, Without<LineText>)>,
) {
    for line in lines.read() {
        line_text.0 = match &line.speaker {
            Some(speaker) => format!("{speaker}: {}", line.text),
            None => line.text.clone(),
        };
        choices_text.0 = "[Space] continue".to_string();
        *waiting = Waiting::Line(line.runner);
    }
}

fn show_choices(
    mut choices: MessageReader<DialogueChoices>,
    mut waiting: ResMut<Waiting>,
    mut choices_text: Single<&mut Text, With<ChoicesText>>,
) {
    for message in choices.read() {
        choices_text.0 = message
            .choices
            .iter()
            .map(|choice| format!("[{}] {}", choice.index + 1, choice.text))
            .collect::<Vec<_>>()
            .join("\n");
        *waiting = Waiting::Choices {
            runner: message.runner,
            count: message.choices.len(),
        };
    }
}

fn show_finished(
    mut finished: MessageReader<DialogueFinished>,
    mut waiting: ResMut<Waiting>,
    mut line_text: Single<&mut Text, With<LineText>>,
    mut choices_text: Single<&mut Text, (With<ChoicesText>, Without<LineText>)>,
) {
    for _ in finished.read() {
        line_text.0 = "End of dialogue.".to_string();
        choices_text.0 = String::new();
        *waiting = Waiting::Nothing;
    }
}

fn handle_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut waiting: ResMut<Waiting>,
    mut resume: MessageWriter<ResumeDialogue>,
) {
    match *waiting {
        Waiting::Nothing => (),
        Waiting::Line(runner) => {
            if keys.just_pressed(KeyCode::Space) {
                resume.write(ResumeDialogue {
                    runner,
                    input: ResumeInput::Advance,
                });
                *waiting = Waiting::Nothing;
            }
        }
        Waiting::Choices { runner, count } => {
            for (index, key) in CHOICE_KEYS.iter().take(count).enumerate() {
                if keys.just_pressed(*key) {
                    resume.write(ResumeDialogue {
                        runner,
                        input: ResumeInput::Select(index),
                    });
                    *waiting = Waiting::Nothing;
                    break;
                }
            }
        }
    }
}
