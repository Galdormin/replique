# Showing a dialogue

A runner plays its dialogue until it needs the game, then stops and says why with a message. The game answers with a `ResumeDialogue`, and the runner carries on to the next stop.

| The runner sends   | When it reaches         | The game answers with                                 |
| ------------------ | ----------------------- | ----------------------------------------------------- |
| `DialogueLine`     | A line                  | `ResumeInput::Advance`                                |
| `DialogueChoices`  | A group of choices      | `ResumeInput::Select`                                 |
| `DialogueCommand`  | A `>>` command          | Depends if it is awaited, see [Commands](commands.md) |
| `DialogueFinished` | The end of the dialogue | Nothing                                               |

## Lines

A `DialogueLine` holds the speaker, if there is one, the text with its inline expressions already replaced by their values, and the [tags](tags.md) of the line.

A game usually shows the line, then waits for the player before moving on. The message has to be kept until then, or at least its **token**:

```rust
# use bevy::prelude::*;
# use bevy_replique::prelude::*;
/// The line on screen, waiting for the player.
#[derive(Resource, Default)]
struct CurrentLine(Option<DialogueToken>);

fn show_line(mut lines: MessageReader<DialogueLine>, mut current: ResMut<CurrentLine>) {
    for line in lines.read() {
        match &line.speaker {
            Some(speaker) => println!("{speaker}: {}", line.text),
            None => println!("{}", line.text),
        }
        current.0 = Some(line.token);
    }
}

fn next_line(
    keys: Res<ButtonInput<KeyCode>>,
    mut current: ResMut<CurrentLine>,
    mut resume: MessageWriter<ResumeDialogue>,
) {
    if !keys.just_pressed(KeyCode::Space) {
        return;
    }

    if let Some(token) = current.0.take() {
        resume.write(ResumeDialogue {
            token,
            input: ResumeInput::Advance,
        });
    }
}
# let mut app = App::new();
# app.init_resource::<CurrentLine>().add_systems(Update, (show_line, next_line));
```

## Tokens

Every message of a runner carries a `DialogueToken`, and a `ResumeDialogue` has to hand that same token back. A token is good for one stop only: the runner makes a new one each time it moves.

An answer that arrives late is therefore dropped instead of advancing whatever the dialogue has reached in the meantime. Pressing the key twice on a line does not skip the next one, and an animation that ends after the player skipped it does nothing.

`token.runner()` gives the entity of the runner, which is how several dialogues running at once are told apart.

## Choices

A `DialogueChoices` holds the whole group. Each `DialogueChoice` has its `text`, its `tags`, its `index` in the group, and whether it is `available`.

```replique
:= start
[let $gold = 3]
Guard: Halt!
-> Talk your way in
    Guard: Fine, go ahead.
-> Bribe the guard [if $gold >= 10]
    Guard: I saw nothing.
---
```

A choice whose [condition](../syntax/choices.md#conditional-choices) does not hold is still in the list, with `available` set to `false`: the game decides whether to hide it or to show it greyed out. Its `index` is the one to send back, whatever is shown.

```rust
# use bevy::prelude::*;
# use bevy_replique::prelude::*;
/// The choices on screen, waiting for the player.
#[derive(Resource, Default)]
struct CurrentChoices(Option<DialogueChoices>);

fn show_choices(mut groups: MessageReader<DialogueChoices>, mut current: ResMut<CurrentChoices>) {
    for group in groups.read() {
        for choice in &group.choices {
            if choice.available {
                println!("[{}] {}", choice.index + 1, choice.text);
            } else {
                println!("[-] {} (unavailable)", choice.text);
            }
        }
        current.0 = Some(group.clone());
    }
}

fn pick_choice(
    keys: Res<ButtonInput<KeyCode>>,
    mut current: ResMut<CurrentChoices>,
    mut resume: MessageWriter<ResumeDialogue>,
) {
    const KEYS: [KeyCode; 3] = [KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3];

    let Some(group) = &current.0 else {
        return;
    };

    // Only an available choice can be picked.
    let picked = group
        .choices
        .iter()
        .find(|choice| choice.available && KEYS.get(choice.index).is_some_and(|key| keys.just_pressed(*key)));

    if let Some(choice) = picked {
        resume.write(ResumeDialogue {
            token: group.token,
            input: ResumeInput::Select(choice.index),
        });
        current.0 = None;
    }
}
# let mut app = App::new();
# app.init_resource::<CurrentChoices>().add_systems(Update, (show_choices, pick_choice));
```

Selecting a choice that is not available, or an index outside of the group, is an error: only ever send the index of an available choice.

## End

`DialogueFinished` is sent when the dialogue reaches the end of a node with no `<=>` to come back to, or a `=> END`. It carries the entity of the runner rather than a token, since there is nothing left to resume.

```rust
# use bevy::prelude::*;
# use bevy_replique::prelude::*;
fn close_dialogue(mut finished: MessageReader<DialogueFinished>, mut commands: Commands) {
    for message in finished.read() {
        // Done with this dialogue: the runner can go.
        commands.entity(message.runner).despawn();
    }
}
# let mut app = App::new();
# app.add_systems(Update, close_dialogue);
```

Despawning the runner is not required. A finished runner can be given a new `StartDialogue` instead, and it keeps the variables the dialogue has set.

## Schedule

The runners move in `PostUpdate`, in the `DialogueSystem::Runner` set, and the commands run right after in `DialogueSystem::Commands`. A `ResumeDialogue` written during `Update` is therefore answered in the same frame.
