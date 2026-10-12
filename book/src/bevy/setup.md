# Getting started

`bevy_replique` is the Bevy plugin for Replique. A `.rep` file is an asset, a running dialogue is an entity, and the game talks to it with messages.

## Installing

```sh
cargo add bevy_replique
```

Which version of Bevy each release works with is listed in the [README](https://github.com/Galdormin/replique#versions).

Everything used in this part of the book comes from the prelude:

```rust
use bevy_replique::prelude::*;
```

## A first dialogue

Save this as `assets/dialogue/hello.rep`:

```replique
:= start
Alice: Hi! How are you?
Bob: Great, and you?
---
```

Three things are needed to play it: the plugin, a **runner** holding the dialogue, and a message telling the runner where to start.

```rust,no_run
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
        match &line.speaker {
            Some(speaker) => println!("{speaker}: {}", line.text),
            None => println!("{}", line.text),
        }

        resume.write(ResumeDialogue {
            token: line.token,
            input: ResumeInput::Advance,
        });
    }
}
```

`RepliquePlugin` registers the asset loader for `.rep` and `.replique` files, the messages, and the systems that run the dialogues.

`DialogueRunner` is a component: the entity that holds it _is_ the running dialogue. It can be started before its asset is loaded, the start is kept pending until the file is there.

`StartDialogue` names the runner and the node to enter. A runner can be started again once its dialogue is finished, on the same node or on another one.

## When a file has errors

A file with an error does not load: the asset fails, and the error Bevy logs holds the full report, with the line and the reason of each problem.

```text
error[jump-unknown-node]: jump to unknown node `nope`
  --> dialogue/hello.rep:3:1
  |
3 | => nope
  | ^^^^^^^
```

The same report is given by `replique check` from the command line and by the language server in the editor, before the game is ever run.

## Examples

The repository has five [runnable examples](https://github.com/Galdormin/replique/tree/main/crates/bevy_replique/examples), from a minimal UI to custom commands, functions, features and several dialogues at once:

```sh
cargo run -p bevy_replique --example bevy_simple
```
