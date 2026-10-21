use bevy::{
    prelude::*,
    text::{FontSource, FontStyle, FontWeight, GenericFontFamily},
};
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

const LINE_SIZE: f32 = 28.0;
const CHOICE_SIZE: f32 = 22.0;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(AssetPlugin {
            // Resolved against `CARGO_MANIFEST_DIR`, so the assets shipped with
            // the crate are found wherever the example runs from.
            file_path: "assets".to_string(),
            ..default()
        }))
        .add_plugins(RepliquePlugin)
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
    Line(DialogueToken),
    /// A [`DialogueChoices`] is displayed, waiting for [`ResumeInput::Select`].
    Choices { token: DialogueToken, count: usize },
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

    let dialogue: Handle<RepliqueDialogue> = asset_server.load("dialogue/markup.rep");
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
            (Text::new("..."), font(LINE_SIZE, false, false), LineText),
            (
                Text::new(""),
                font(CHOICE_SIZE, false, false),
                TextColor(Color::srgb(0.7, 0.7, 0.8)),
                ChoicesText,
            ),
        ],
    ));
}

/// The font of a piece of text. The system sans-serif font is asked for, since
/// the font Bevy embeds has no bold nor italic face.
fn font(size: f32, bold: bool, italic: bool) -> TextFont {
    TextFont {
        font: FontSource::Generic(GenericFontFamily::SansSerif),
        font_size: size.into(),
        weight: if bold {
            FontWeight::BOLD
        } else {
            FontWeight::NORMAL
        },
        style: if italic {
            FontStyle::Italic
        } else {
            FontStyle::Normal
        },
        ..default()
    }
}

/// Adds a [`TextSpan`] to `root` for each run of text of `pieces`, in bold
/// under a `<b>` and in italic under an `<i>`.
///
/// A markup standing on its own, such as `<pause/>`, is meant for a
/// typewriter, which this example does not have: it is skipped.
fn spawn_pieces(commands: &mut Commands, root: Entity, pieces: Vec<Piece>, size: f32) {
    commands.entity(root).with_children(|spans| {
        for piece in pieces {
            let Piece::Text(segment) = piece else {
                continue;
            };
            spans.spawn((
                TextSpan::new(segment.text),
                font(size, segment.has("b"), segment.has("i")),
            ));
        }
    });
}

fn show_line(
    mut commands: Commands,
    mut lines: MessageReader<DialogueLine>,
    mut waiting: ResMut<Waiting>,
    line_text: Single<(Entity, &mut Text), With<LineText>>,
    choices_text: Single<(Entity, &mut Text), (With<ChoicesText>, Without<LineText>)>,
) {
    let (line_root, mut line_prefix) = line_text.into_inner();
    let (choices_root, mut choices_prefix) = choices_text.into_inner();

    for line in lines.read() {
        // The speaker goes in the root text, and the line in its spans.
        line_prefix.0 = match &line.speaker {
            Some(speaker) => format!("{speaker}: "),
            None => String::new(),
        };
        commands.entity(line_root).despawn_children();
        spawn_pieces(&mut commands, line_root, line.pieces(), LINE_SIZE);

        choices_prefix.0 = "[Space] continue".to_string();
        commands.entity(choices_root).despawn_children();

        *waiting = Waiting::Line(line.token);
    }
}

fn show_choices(
    mut commands: Commands,
    mut choices: MessageReader<DialogueChoices>,
    mut waiting: ResMut<Waiting>,
    choices_text: Single<(Entity, &mut Text), With<ChoicesText>>,
) {
    let (root, mut prefix) = choices_text.into_inner();

    for message in choices.read() {
        prefix.0 = String::new();
        commands.entity(root).despawn_children();

        // One span for the number of each choice, then the spans of its text.
        for choice in &message.choices {
            let separator = if choice.index == 0 { "" } else { "\n" };
            commands.entity(root).with_child((
                TextSpan::new(format!("{separator}[{}] ", choice.index + 1)),
                font(CHOICE_SIZE, false, false),
            ));
            spawn_pieces(&mut commands, root, choice.pieces(), CHOICE_SIZE);
        }

        *waiting = Waiting::Choices {
            token: message.token,
            count: message.choices.len(),
        };
    }
}

fn show_finished(
    mut commands: Commands,
    mut finished: MessageReader<DialogueFinished>,
    mut waiting: ResMut<Waiting>,
    line_text: Single<(Entity, &mut Text), With<LineText>>,
    choices_text: Single<(Entity, &mut Text), (With<ChoicesText>, Without<LineText>)>,
) {
    let (line_root, mut line_prefix) = line_text.into_inner();
    let (choices_root, mut choices_prefix) = choices_text.into_inner();

    for _ in finished.read() {
        line_prefix.0 = "End of dialogue.".to_string();
        commands.entity(line_root).despawn_children();
        choices_prefix.0 = String::new();
        commands.entity(choices_root).despawn_children();
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
        Waiting::Line(token) => {
            if keys.just_pressed(KeyCode::Space) {
                resume.write(ResumeDialogue {
                    token,
                    input: ResumeInput::Advance,
                });
                *waiting = Waiting::Nothing;
            }
        }
        Waiting::Choices { token, count } => {
            for (index, key) in CHOICE_KEYS.iter().take(count).enumerate() {
                if keys.just_pressed(*key) {
                    resume.write(ResumeDialogue {
                        token,
                        input: ResumeInput::Select(index),
                    });
                    *waiting = Waiting::Nothing;
                    break;
                }
            }
        }
    }
}
