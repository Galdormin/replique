use bevy::{prelude::*, sprite::Anchor, text::TextBounds};
use bevy_replique::prelude::*;

/// Node every dialogue starts on.
const START_NODE: &str = "start";

/// Height of the top of the ground.
const GROUND_Y: f32 = -200.0;
/// How far the player can walk on either side.
const WALK_LIMIT: f32 = 580.0;
/// Pixels walked in a second.
const WALK_SPEED: f32 = 260.0;
/// How close the player has to be to talk to a character.
const TALK_RANGE: f32 = 90.0;

const CAPSULE_RADIUS: f32 = 22.0;
const CAPSULE_LENGTH: f32 = 44.0;
/// Height of the center of a capsule standing on the ground.
const STANDING_Y: f32 = GROUND_Y + CAPSULE_LENGTH / 2.0 + CAPSULE_RADIUS;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(AssetPlugin {
            // Resolved against `CARGO_MANIFEST_DIR`, so the assets shipped with
            // the crate are found wherever the example runs from.
            file_path: "assets".to_string(),
            ..default()
        }))
        .add_plugins(RepliquePlugin)
        // `Guard.Door` is the door of the guard who is speaking.
        .register_replique_feature_value::<Door>("Door")
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (walk, talk, show_line, show_finished, show_prompt).chain(),
        )
        .run();
}

/// The capsule the keys move.
#[derive(Component)]
struct Player;

/// The door a guard stands at, which the dialogue reads as `Guard.Door`.
#[derive(RepliqueValue, Component, Debug, Clone, Copy)]
enum Door {
    East,
    West,
}

/// Where the dialogue of a character is at, kept on the character since each
/// one runs on its own.
#[derive(Component, Default)]
enum Talk {
    /// Not started yet, or over: Space starts it.
    #[default]
    Idle,
    /// Started or resumed, and not answered yet.
    Running,
    /// A [`DialogueLine`] is displayed, waiting for [`ResumeInput::Advance`].
    Line(DialogueToken),
}

/// The text above a character. `.0` is the entity of that character, which is
/// also its runner.
#[derive(Component)]
struct Bubble(Entity);

fn setup(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    commands.spawn(Camera2d);

    let capsule = meshes.add(Capsule2d::new(CAPSULE_RADIUS, CAPSULE_LENGTH));

    // The ground, with its top at `GROUND_Y`.
    commands.spawn((
        Mesh2d(meshes.add(Rectangle::new(2.0 * WALK_LIMIT + 120.0, 16.0))),
        MeshMaterial2d(materials.add(Color::srgb(0.3, 0.3, 0.36))),
        Transform::from_xyz(0.0, GROUND_Y - 8.0, 0.0),
    ));

    commands.spawn((
        Player,
        Mesh2d(capsule.clone()),
        MeshMaterial2d(materials.add(Color::srgb(0.95, 0.95, 0.95))),
        Transform::from_xyz(-200.0, STANDING_Y, 1.0),
    ));

    let guard: Handle<RepliqueDialogue> = asset_server.load("dialogue/guard.rep");
    let merchant: Handle<RepliqueDialogue> = asset_server.load("dialogue/merchant.rep");

    // One handle, two runners: the guards share a file, not the runner.
    for (x, door) in [(-440.0, Door::West), (440.0, Door::East)] {
        let character = spawn_character(
            &mut commands,
            capsule.clone(),
            materials.add(Color::srgb(0.62, 0.75, 0.94)),
            x,
            guard.clone(),
        );

        // Both are called `Guard`, each for its own runner only.
        // that is how the same line reads a different door.
        commands
            .entity(character)
            .insert((RepliqueSubject::new("Guard").only_for(character), door));
    }

    spawn_character(
        &mut commands,
        capsule,
        materials.add(Color::srgb(0.92, 0.68, 0.72)),
        0.0,
        merchant,
    );

    commands.spawn((
        Text::new("[A/D] walk    [Space] talk, then continue"),
        TextFont::from_font_size(20.0),
        TextColor(Color::srgb(0.7, 0.7, 0.8)),
        Node {
            position_type: PositionType::Absolute,
            bottom: px(24),
            left: px(24),
            ..default()
        },
    ));
}

/// A capsule standing at `x` that plays `dialogue`, with the text it says
/// above it. The entity it gives back is the character and its runner alike.
fn spawn_character(
    commands: &mut Commands,
    capsule: Handle<Mesh>,
    material: Handle<ColorMaterial>,
    x: f32,
    dialogue: Handle<RepliqueDialogue>,
) -> Entity {
    let character = commands
        .spawn((
            DialogueRunner::new(dialogue),
            Talk::default(),
            Mesh2d(capsule),
            MeshMaterial2d(material),
            Transform::from_xyz(x, STANDING_Y, 0.0),
        ))
        .id();

    commands.spawn((
        Bubble(character),
        ChildOf(character),
        Text2d::new(""),
        TextFont::from_font_size(20.0),
        TextLayout::justify(Justify::Center),
        TextBounds::new_horizontal(300.0),
        Anchor::BOTTOM_CENTER,
        Transform::from_xyz(0.0, CAPSULE_LENGTH / 2.0 + CAPSULE_RADIUS + 12.0, 2.0),
    ));

    character
}

fn walk(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut player: Single<&mut Transform, With<Player>>,
) {
    let mut direction = 0.0;
    if keys.any_pressed([KeyCode::KeyA, KeyCode::ArrowLeft]) {
        direction -= 1.0;
    }
    if keys.any_pressed([KeyCode::KeyD, KeyCode::ArrowRight]) {
        direction += 1.0;
    }

    let x = player.translation.x + direction * WALK_SPEED * time.delta_secs();
    player.translation.x = x.clamp(-WALK_LIMIT, WALK_LIMIT);
}

/// The character the player stands next to, if any.
fn nearby(player: &Transform, characters: impl Iterator<Item = (Entity, f32)>) -> Option<Entity> {
    characters
        .map(|(character, x)| (character, (x - player.translation.x).abs()))
        .filter(|(_, distance)| *distance <= TALK_RANGE)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(character, _)| character)
}

/// Space talks to the character nearby.
fn talk(
    keys: Res<ButtonInput<KeyCode>>,
    player: Single<&Transform, With<Player>>,
    mut characters: Query<(Entity, &Transform, &mut Talk)>,
    mut start: MessageWriter<StartDialogue>,
    mut resume: MessageWriter<ResumeDialogue>,
) {
    if !keys.just_pressed(KeyCode::Space) {
        return;
    }

    let positions = characters
        .iter()
        .map(|(character, at, _)| (character, at.translation.x));
    let Some(character) = nearby(&player, positions) else {
        return;
    };
    let Ok((runner, _, mut talk)) = characters.get_mut(character) else {
        return;
    };

    match *talk {
        // A runner whose dialogue is over starts again like a new one.
        Talk::Idle => {
            start.write(StartDialogue {
                runner,
                node: START_NODE.into(),
            });
        }
        Talk::Line(token) => {
            resume.write(ResumeDialogue {
                token,
                input: ResumeInput::Advance,
            });
        }
        Talk::Running => return,
    }
    *talk = Talk::Running;
}

/// Writes each line above the character whose runner sent it.
fn show_line(
    mut lines: MessageReader<DialogueLine>,
    mut characters: Query<&mut Talk>,
    mut bubbles: Query<(&Bubble, &mut Text2d)>,
) {
    for line in lines.read() {
        let runner = line.token.runner();

        if let Ok(mut talk) = characters.get_mut(runner) {
            *talk = Talk::Line(line.token);
        }
        for (Bubble(character), mut text) in &mut bubbles {
            if *character == runner {
                text.0 = line.text.clone();
            }
        }
    }
}

fn show_finished(
    mut finished: MessageReader<DialogueFinished>,
    mut characters: Query<&mut Talk>,
    mut bubbles: Query<(&Bubble, &mut Text2d)>,
) {
    for message in finished.read() {
        if let Ok(mut talk) = characters.get_mut(message.runner) {
            *talk = Talk::Idle;
        }
        for (Bubble(character), mut text) in &mut bubbles {
            if *character == message.runner {
                text.0.clear();
            }
        }
    }
}

fn show_prompt(
    player: Single<&Transform, With<Player>>,
    characters: Query<(Entity, &Transform, &Talk)>,
    mut bubbles: Query<(&Bubble, &mut Text2d)>,
) {
    let positions = characters
        .iter()
        .map(|(character, at, _)| (character, at.translation.x));
    let near = nearby(&player, positions);

    for (Bubble(character), mut text) in &mut bubbles {
        let Ok((_, _, Talk::Idle)) = characters.get(*character) else {
            continue;
        };

        let prompt = if near == Some(*character) {
            "[Space]"
        } else {
            ""
        };
        if text.0 != prompt {
            text.0 = prompt.to_string();
        }
    }
}
