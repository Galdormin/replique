//! Portraits driven by the `>>` commands of `assets/dialogue/custom_command.rep`.
//!
//! The dialogue casts its own scene: `>> add_scene(Alice, Bob)` brings the
//! characters in, `>> remove_scene(Alice)` sends them away, and each line is
//! displayed next to the portrait of whoever speaks it.
//!
//! It is also where a custom argument type earns its keep. [`Character`]
//! implements [`FromValue`], so a word the cast does not know is refused
//! before it reaches the game, and [`CharactersParams`] derives
//! [`RepliqueArgs`] so that a command takes as many characters as the writer
//! put.
//!
//! Space advances a line. This dialogue has no choice to make.
//!
//! ```sh
//! cargo run -p bevy_replique --example bevy_custom_command
//! ```

use bevy::prelude::*;
use bevy_replique::prelude::*;

/// Node the dialogue starts on.
const START_NODE: &str = "start";

/// Side of a portrait, and width of the column they share.
const PORTRAIT: f32 = 96.0;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(AssetPlugin {
            // The assets live at the root of the workspace, not in the crate.
            file_path: "../../assets".to_string(),
            ..default()
        }))
        .add_plugins(RepliquePLugin)
        .add_dialogue_command(add_scene)
        .add_dialogue_command(remove_scene)
        .init_resource::<Waiting>()
        .add_systems(Startup, setup)
        .add_systems(Update, (show_line, show_finished, handle_input))
        .run();
}

/// Represents character in the dialogue
#[derive(RepliqueValue, Component, Debug, Clone, Copy, PartialEq, Eq)]
enum Character {
    Alice,
    Bob,
    Caroline,
}

impl Character {
    /// The name the dialogue writes, both as a speaker and as an argument.
    fn from_name(name: &str) -> Option<Self> {
        match name {
            "Alice" => Some(Self::Alice),
            "Bob" => Some(Self::Bob),
            "Caroline" => Some(Self::Caroline),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Alice => "Alice",
            Self::Bob => "Bob",
            Self::Caroline => "Caroline",
        }
    }

    /// Stands in for a portrait.
    fn color(self) -> Color {
        match self {
            Self::Alice => Color::srgb(0.92, 0.68, 0.72),
            Self::Bob => Color::srgb(0.62, 0.75, 0.94),
            Self::Caroline => Color::srgb(0.70, 0.88, 0.70),
        }
    }
}

/// As many characters as the command was given.
///
/// A tuple would pin their number down, which is what makes this one worth a
/// [`FromDialogueArgs`] of its own: `>> add_scene(Alice, Bob)` and
/// `>> add_scene(Caroline)` both fit. `#[variadic]` is what the derive reads
/// as "every argument left".
#[derive(RepliqueArgs)]
struct CharactersParams(#[variadic] Vec<Character>);

/// `>> add_scene(Alice, Bob)`
#[replique_command]
fn add_scene(
    In(CharactersParams(characters)): In<CharactersParams>,
    mut commands: Commands,
    stage: Single<Entity, With<Stage>>,
    on_stage: Query<&Character>,
) {
    for character in characters {
        if on_stage.iter().any(|on_stage| *on_stage == character) {
            continue;
        }

        commands.entity(*stage).with_child((
            character,
            Node {
                align_items: AlignItems::Center,
                column_gap: px(16),
                ..default()
            },
            children![
                (
                    Node {
                        width: px(PORTRAIT),
                        height: px(PORTRAIT),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    BackgroundColor(character.color()),
                    children![(
                        Text::new(character.name()),
                        TextFont::from_font_size(16.0),
                        TextColor(Color::BLACK),
                    )],
                ),
                (
                    Text::new(""),
                    TextFont::from_font_size(24.0),
                    LineText(character),
                ),
            ],
        ));
    }
}

/// `>> remove_scene(Alice)`
#[replique_command]
fn remove_scene(
    In(CharactersParams(characters)): In<CharactersParams>,
    mut commands: Commands,
    on_stage: Query<(Entity, &Character)>,
) {
    for (entity, character) in &on_stage {
        if characters.contains(character) {
            commands.entity(entity).despawn();
        }
    }
}

/// What the runner is currently waiting for.
///
/// This dialogue has no choice group, so a line is the only thing to answer.
#[derive(Resource, Default)]
enum Waiting {
    /// Nothing to answer: the dialogue is loading, running or finished.
    #[default]
    Nothing,
    /// A [`DialogueLine`] is displayed, waiting for [`ResumeInput::Advance`].
    Line(DialogueToken),
}

/// Column the portraits are added to.
#[derive(Component)]
struct Stage;

/// Text node facing the portrait of `.0`.
#[derive(Component)]
struct LineText(Character);

/// Text node showing what to press.
#[derive(Component)]
struct HintText;

fn setup(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut start: MessageWriter<StartDialogue>,
) {
    commands.spawn(Camera2d);

    let dialogue: Handle<RepliqueDialogue> = asset_server.load("dialogue/custom_command.rep");
    let runner = commands.spawn(DialogueRunner::new(dialogue)).id();
    start.write(StartDialogue {
        runner,
        node: START_NODE.into(),
    });

    commands.spawn((
        Node {
            width: percent(100),
            height: percent(100),
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::SpaceBetween,
            padding: UiRect::all(px(32)),
            ..default()
        },
        children![
            // The portraits stack down the left edge, each with its line
            // beside it.
            (
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: px(16),
                    ..default()
                },
                Stage,
            ),
            (
                Text::new("[Space] continue"),
                TextFont::from_font_size(20.0),
                TextColor(Color::srgb(0.7, 0.7, 0.8)),
                HintText,
            ),
        ],
    ));
}

/// Puts the line in front of the portrait of whoever speaks it, and clears the
/// others so that only one character talks at a time.
fn show_line(
    mut lines: MessageReader<DialogueLine>,
    mut waiting: ResMut<Waiting>,
    mut texts: Query<(&LineText, &mut Text)>,
) {
    for line in lines.read() {
        let speaking = line.speaker.as_deref().and_then(Character::from_name);

        for (LineText(character), mut text) in &mut texts {
            text.0 = match speaking {
                Some(speaking) if speaking == *character => line.text.clone(),
                _ => String::new(),
            };
        }

        *waiting = Waiting::Line(line.token);
    }
}

fn show_finished(
    mut finished: MessageReader<DialogueFinished>,
    mut waiting: ResMut<Waiting>,
    mut hint: Single<&mut Text, With<HintText>>,
) {
    for _ in finished.read() {
        hint.0 = "End of dialogue.".to_string();
        *waiting = Waiting::Nothing;
    }
}

fn handle_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut waiting: ResMut<Waiting>,
    mut resume: MessageWriter<ResumeDialogue>,
) {
    if let Waiting::Line(token) = *waiting
        && keys.just_pressed(KeyCode::Space)
    {
        resume.write(ResumeDialogue {
            token,
            input: ResumeInput::Advance,
        });
        *waiting = Waiting::Nothing;
    }
}
