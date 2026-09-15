//! Characters read from the world by the expressions of
//! `assets/dialogue/custom_function.rep`.
//!
//! [`Stats`] comes back as a dict, so the dialogue reads `$stat.hp` the same
//! way it would read any other. A character nobody spawned makes `get_stat`
//! answer an error.
//!
//! Space advances a line. This dialogue has no choice to make.
//!
//! ```sh
//! cargo run -p bevy_replique --example bevy_custom_function
//! ```
//!
//! [`add_dialogue_function_named`]: bevy_replique::function::DialogueFunctionAppExt::add_dialogue_function_named

use bevy::prelude::*;
use bevy_replique::prelude::*;

/// Node the dialogue starts on.
const START_NODE: &str = "start";

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(AssetPlugin {
            // Resolved against `CARGO_MANIFEST_DIR`, so the assets shipped with
            // the crate are found wherever the example runs from.
            file_path: "assets".to_string(),
            ..default()
        }))
        .add_plugins(RepliquePlugin)
        // Read the world, and answer a value.
        .add_dialogue_function(get_stat)
        .add_dialogue_function(party_size)
        .add_dialogue_function(richest)
        // Writes to the world, and answers nothing.
        .add_dialogue_command(hurt)
        .init_resource::<Waiting>()
        .add_systems(Startup, setup)
        .add_systems(Update, (show_line, show_stats, show_finished, handle_input))
        .run();
}

/// Someone the dialogue can name, as a speaker or as an argument.
#[derive(RepliqueValue, Component, Debug, Clone, Copy, PartialEq, Eq)]
enum Character {
    Alice,
    Bob,
}

impl Character {
    fn name(self) -> &'static str {
        match self {
            Self::Alice => "Alice",
            Self::Bob => "Bob",
        }
    }
}

/// What the dialogue reads as `$stat.hp` and `$stat.gold`.
#[derive(RepliqueValue, Component, Debug, Clone, Copy)]
struct Stats {
    hp: i64,
    gold: i64,
}

/// `[let $stat = get_stat(Alice)]`
#[replique_function]
fn get_stat(
    In((who,)): In<(Character,)>,
    characters: Query<(&Character, &Stats)>,
) -> Result<Stats, String> {
    characters
        .iter()
        .find(|(character, _)| **character == who)
        .map(|(_, stats)| stats.to_owned())
        .ok_or_else(|| format!("{} is not on the scene", who.name()))
}

/// `[party_size()]`
#[replique_function]
fn party_size(In(()): In<()>, characters: Query<&Character>) -> i64 {
    characters.iter().count() as i64
}

/// `[richest()]`
#[replique_function]
fn richest(In(()): In<()>, characters: Query<(&Character, &Stats)>) -> Result<Character, String> {
    characters
        .iter()
        .max_by_key(|(_, stats)| stats.gold)
        .map(|(character, _)| *character)
        .ok_or_else(|| "nobody is on the scene".to_owned())
}

/// `>> hurt(Alice, 8)`
#[replique_command]
fn hurt(In((who, damage)): In<(Character, i64)>, mut characters: Query<(&Character, &mut Stats)>) {
    for (character, mut stats) in &mut characters {
        if *character == who {
            stats.hp = (stats.hp - damage).max(0);
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

/// Text node showing the stats of `.0`, so the `>> hurt` is visible.
#[derive(Component)]
struct StatText(Character);

/// Text node showing the line being said.
#[derive(Component)]
struct LineText;

/// Text node showing what to press.
#[derive(Component)]
struct HintText;

fn setup(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut start: MessageWriter<StartDialogue>,
) {
    commands.spawn(Camera2d);

    commands.spawn((Character::Alice, Stats { hp: 14, gold: 3 }));
    commands.spawn((Character::Bob, Stats { hp: 11, gold: 12 }));

    let dialogue: Handle<RepliqueDialogue> = asset_server.load("dialogue/custom_function.rep");
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
            // The sheet the functions read, so what they answer can be
            // checked against it.
            (
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: px(8),
                    ..default()
                },
                children![
                    (
                        Text::new(""),
                        TextFont::from_font_size(20.0),
                        TextColor(Color::srgb(0.92, 0.68, 0.72)),
                        StatText(Character::Alice),
                    ),
                    (
                        Text::new(""),
                        TextFont::from_font_size(20.0),
                        TextColor(Color::srgb(0.62, 0.75, 0.94)),
                        StatText(Character::Bob),
                    ),
                ],
            ),
            (Text::new(""), TextFont::from_font_size(28.0), LineText),
            (
                Text::new("[Space] continue"),
                TextFont::from_font_size(20.0),
                TextColor(Color::srgb(0.7, 0.7, 0.8)),
                HintText,
            ),
        ],
    ));
}

/// Keeps the sheet in step with the world, so the `>> hurt` shows up next to
/// what `get_stat` answers after it.
fn show_stats(characters: Query<(&Character, &Stats)>, mut texts: Query<(&StatText, &mut Text)>) {
    for (StatText(who), mut text) in &mut texts {
        let Some((_, stats)) = characters.iter().find(|(c, _)| *c == who) else {
            continue;
        };

        text.0 = format!("{}  hp {}  gold {}", who.name(), stats.hp, stats.gold);
    }
}

fn show_line(
    mut lines: MessageReader<DialogueLine>,
    mut waiting: ResMut<Waiting>,
    mut text: Single<&mut Text, With<LineText>>,
) {
    for line in lines.read() {
        text.0 = match &line.speaker {
            Some(speaker) => format!("{speaker}: {}", line.text),
            None => line.text.clone(),
        };

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
