# Replique

[![Crates.io](https://img.shields.io/crates/v/replique.svg?label=replique)](https://crates.io/crates/replique)
[![Docs.rs](https://img.shields.io/docsrs/replique?label=docs.rs)](https://docs.rs/replique)
[![Crates.io](https://img.shields.io/crates/v/bevy_replique.svg?label=bevy_replique)](https://crates.io/crates/bevy_replique)
[![Docs.rs](https://img.shields.io/docsrs/bevy_replique?label=docs.rs)](https://docs.rs/bevy_replique)
[![Book](https://img.shields.io/badge/book-read-blue)](https://galdormin.github.io/replique/)
[![License](https://img.shields.io/crates/l/replique.svg)](#license)

Replique is a pure Rust dialogue system, with first-class support for [Bevy](https://bevyengine.org). Dialogues are written in plain text files, with branching choices, variables, conditions and loops, and they reach into the game through commands, functions and tags. A command line checker and a language server report mistakes before the game is ever run.

It is inspired by [YarnSpinner](https://www.yarnspinner.dev), [YarnSpinner-Rust](https://github.com/YarnSpinnerTool/YarnSpinner-Rust) and [Godot Dialogue Manager](https://github.com/nathanhoad/godot_dialogue_manager).

**[Read the Replique Book](https://galdormin.github.io/replique/)** to learn the dialogue language and how to use it in a Bevy game.

## A dialogue

```replique
:= start
[let $gold = 12]
// `Merchant.Price` queries the game directly: it is the `Price` component
// of the entity named `Merchant`, read at the moment the line is played.
Merchant: Welcome, traveller! A lantern is [Merchant.Price] coins. #happy
-> What do you sell?
    <=> wares
-> Buy the lantern [if $gold >= Merchant.Price]
    [let $gold = $gold - Merchant.Price]
    >> give("lantern")
    Merchant: A fine choice, [$gold] coins left for you.
-> Leave
    => END
Merchant: Safe travels!
---

:= wares
Merchant: Lanterns, rope, and the best apples in town.
// `has` asks whether that same entity holds a `Tired` component.
[if Merchant has Tired]
    Merchant: But I am closing soon.
---
```

A file is made of nodes. This one reads the components of an entity of the game without any glue code, and also holds a variable, lines with values inserted in them, a tag, a group of choices of which one is only available with enough gold, a command for the game to run, and a jump to another node that comes back.

## In Bevy

```sh
cargo add bevy_replique
```

A `.rep` file is an asset, a running dialogue is an entity, and the game talks to it with messages:

```rust
use bevy::prelude::*;
use bevy_replique::prelude::*;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(RepliquePlugin)
        .add_systems(Startup, setup)
        .add_systems(Update, show_line)
        .run();
}

fn setup(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut start: MessageWriter<StartDialogue>,
) {
    let dialogue: Handle<RepliqueDialogue> = assets.load("dialogue/hello.rep");
    let runner = commands.spawn(DialogueRunner::new(dialogue)).id();

    start.write(StartDialogue {
        runner,
        node: "start".into(),
    });
}

/// Prints every line, then asks the runner to carry on.
fn show_line(
    mut lines: MessageReader<DialogueLine>,
    mut resume: MessageWriter<ResumeDialogue>,
) {
    for line in lines.read() {
        println!("{}: {}", line.speaker.as_deref().unwrap_or("-"), line.text);

        resume.write(ResumeDialogue {
            token: line.token,
            input: ResumeInput::Advance,
        });
    }
}
```

Choices, commands, functions and the rest are covered in the [book](https://galdormin.github.io/replique/bevy/setup.html), and the [examples](crates/bevy_replique/examples) are runnable:

```sh
cargo run -p bevy_replique --example bevy_simple
```

## Crates

| Crate                                   | Description                                     |
| --------------------------------------- | ----------------------------------------------- |
| [`replique`](crates/replique)           | The engine-agnostic parser and dialogue runtime |
| [`bevy_replique`](crates/bevy_replique) | Bevy plugin built on top of `replique`          |
| [`replique-cli`](crates/replique-cli)   | `replique check`, to lint dialogue files        |
| [`replique-lsp`](crates/replique-lsp)   | Language server, for diagnostics as you type    |

## Versions

| Replique | Bevy |
| -------- | ---- |
| 0.3      | 0.20 |
| 0.2      | 0.19 |
| 0.1      | 0.19 |

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
