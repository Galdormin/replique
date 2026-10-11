# Tags

The [tags](../syntax/tags.md) of a line or of a choice reach the game apart from the text, in the `tags` field of `DialogueLine` and of `DialogueChoice`. What they mean is up to the game.

```replique
:= start
Alice: #angry Give it back! #sound:alice_01
-> #hostile Refuse
-> Give it back #cost:3
---
```

A `Tag` has a `name`, and a `value` when one is written after the `:`. The value is kept as text, whatever it looks like: `#volume:0.5` holds the string `0.5`.

## On a line

```rust
# use bevy::prelude::*;
# use bevy_replique::prelude::*;
fn react_to_tags(mut lines: MessageReader<DialogueLine>) {
    for line in lines.read() {
        for tag in &line.tags {
            match (tag.name.as_str(), tag.value.as_deref()) {
                // `#angry`
                ("angry", None) => info!("{:?} is angry", line.speaker),
                // `#sound:alice_01`
                ("sound", Some(sound)) => info!("playing {sound}"),
                // `#volume:0.5`
                ("volume", Some(volume)) => match volume.parse::<f32>() {
                    Ok(volume) => info!("volume set to {volume}"),
                    Err(_) => warn!("`{volume}` is not a volume"),
                },
                _ => {}
            }
        }
    }
}
# let mut app = App::new();
# app.add_systems(Update, react_to_tags);
```

The text of the line comes without its tags: here `line.text` is `Give it back!`.

## On a choice

Each choice carries its own tags, which suits what the interface needs to know to draw it: an icon, a cost, a colour.

```rust
# use bevy::prelude::*;
# use bevy_replique::prelude::*;
fn show_choices(mut groups: MessageReader<DialogueChoices>) {
    for group in groups.read() {
        for choice in &group.choices {
            let hostile = choice.tags.iter().any(|tag| tag.name == "hostile");
            let cost = choice
                .tags
                .iter()
                .find(|tag| tag.name == "cost")
                .and_then(|tag| tag.value.as_deref());

            match (hostile, cost) {
                (true, _) => println!("[!] {}", choice.text),
                (false, Some(cost)) => println!("{} ({cost} coins)", choice.text),
                (false, None) => println!("{}", choice.text),
            }
        }
    }
}
# let mut app = App::new();
# app.add_systems(Update, show_choices);
```

## Tags or commands?

A tag describes the line it is on, and arrives with it: a mood, a voice-over file, a portrait to show. A [command](commands.md) is a step of its own, played between two lines, and the dialogue can wait for it. A sound that goes with a line is a tag; a door that opens before the next line is a command.

## Checking the tags

A typo in a tag is silent: `#angy` is a valid tag that the game ignores. Declaring the tags of the project in a `replique.toml` lets `replique check` and the editor report the ones the game does not know. See the [`replique-cli` README](https://github.com/Galdormin/replique/tree/main/crates/replique-cli).
