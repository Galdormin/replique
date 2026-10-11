# Commands

A [command](../syntax/commands.md) is how a dialogue _does_ something in the game. In Bevy, a command is a system.

```replique
:= start
Alice: Take this.
>> give("lantern", 1)
>> set_flag(met_alice)
---
```

```rust
# use bevy::prelude::*;
# use bevy::platform::collections::{HashMap, HashSet};
# use bevy_replique::prelude::*;
#[derive(Resource, Default)]
struct Inventory(HashMap<String, i64>);

#[derive(Resource, Default)]
struct Flags(HashSet<String>);

/// `>> give("lantern", 1)`
#[replique_command]
fn give(In((item, count)): In<(String, i64)>, mut inventory: ResMut<Inventory>) {
    *inventory.0.entry(item).or_default() += count;
}

/// `>> set_flag(met_alice)`
#[replique_command]
fn set_flag(In(flag): In<String>, mut flags: ResMut<Flags>) {
    flags.0.insert(flag);
}

# let mut app = App::new();
# app.init_resource::<Inventory>().init_resource::<Flags>();
app.add_dialogue_command(give)
    .add_dialogue_command(set_flag);
```

`#[replique_command]` declares the system as a command, under the name of the function. `add_dialogue_command` registers it. Apart from its first parameter, a command is an ordinary system: it takes resources, queries and `Commands` like any other.

The dialogue moves on by itself once the command has run, there is no `ResumeDialogue` to send.

To give a command another name than the one of its function, write `#[replique_command(name = "set_flag")]`.

## The name becomes a type

`#[replique_command]` replaces the function by a type of the same name, which is what `add_dialogue_command(give)` is handed. A side effect is that this name can no longer be given to a variable or to a parameter, wherever the command is in scope.

It happens more often than it sounds, because a command tends to be named after the thing it is about, and so does the resource holding that thing. Here `>> inventory()` opens the inventory screen, and `give` can no longer call its own parameter `inventory`:

```rust,compile_fail
# use bevy::prelude::*;
# use bevy::platform::collections::HashMap;
# use bevy_replique::prelude::*;
# #[derive(Resource, Default)]
# struct Inventory(HashMap<String, i64>);
# #[derive(Resource, Default)]
# struct InventoryScreen { open: bool }
/// `>> inventory()`
#[replique_command]
fn inventory(In(()): In<()>, mut screen: ResMut<InventoryScreen>) {
    screen.open = true;
}

/// `>> give("lantern", 1)`
#[replique_command]
fn give(In((item, count)): In<(String, i64)>, mut inventory: ResMut<Inventory>) {
    *inventory.0.entry(item).or_default() += count;
}
```

```text
error[E0530]: function parameters cannot shadow unit structs
   |
 2 | #[replique_command]
   | ------------------- the unit struct `inventory` is defined here
...
10 | fn give(In((item, count)): In<(String, i64)>, mut inventory: ResMut<Inventory>) {
   |                                                   ^^^^^^^^^ cannot be named the same as a unit struct
```

The same goes for a `let inventory = ...` or for a field taken out of a struct under that name. There are two ways out.

**Rename what is bound.** The command keeps the name the dialogue uses, and the parameter takes another one:

```rust
# use bevy::prelude::*;
# use bevy::platform::collections::HashMap;
# use bevy_replique::prelude::*;
# #[derive(Resource, Default)]
# struct Inventory(HashMap<String, i64>);
# #[derive(Resource, Default)]
# struct InventoryScreen { open: bool }
# #[replique_command]
# fn inventory(In(()): In<()>, mut screen: ResMut<InventoryScreen>) {
#     screen.open = true;
# }
/// `>> give("lantern", 1)`
#[replique_command]
fn give(In((item, count)): In<(String, i64)>, mut bag: ResMut<Inventory>) {
    *bag.0.entry(item).or_default() += count;
}
```

**Or rename the function, and give the name of the command in the attribute.** A prefix keeps the Rust names out of the way, and reads well in a file that holds many commands:

```rust
# use bevy::prelude::*;
# use bevy::platform::collections::HashMap;
# use bevy_replique::prelude::*;
# #[derive(Resource, Default)]
# struct Inventory(HashMap<String, i64>);
# #[derive(Resource, Default)]
# struct InventoryScreen { open: bool }
/// `>> inventory()`
#[replique_command(name = "inventory")]
fn cmd_inventory(In(()): In<()>, mut screen: ResMut<InventoryScreen>) {
    screen.open = true;
}

/// `>> give("lantern", 1)`
#[replique_command(name = "give")]
fn cmd_give(In((item, count)): In<(String, i64)>, mut inventory: ResMut<Inventory>) {
    *inventory.0.entry(item).or_default() += count;
}

# let mut app = App::new();
# app.init_resource::<Inventory>().init_resource::<InventoryScreen>();
app.add_dialogue_command(cmd_inventory)
    .add_dialogue_command(cmd_give);
```

## Arguments

What the dialogue writes between the parentheses arrives through `In`, already converted to the types the system asks for.

| The system asks for         | The dialogue writes                         |
| --------------------------- | ------------------------------------------- |
| `In<()>`                    | `>> fade_in()`                              |
| `In<String>`                | `>> set_flag(met_alice)`                    |
| `In<(String, i64)>`         | `>> give("lantern", 1)`                     |
| `In<(String, Option<f32>)>` | `>> play("bell")` or `>> play("bell", 0.5)` |
| `In<DialogueArgs>`          | Any number of arguments, as raw `Value`     |

A tuple takes up to eight arguments. A trailing `Option` makes the last ones optional.

The types are matched as the dialogue writes them:

| Value in the dialogue      | Rust type                                      |
| -------------------------- | ---------------------------------------------- |
| `12`                       | `i8` to `i64`, `u8` to `u64`, `isize`, `usize` |
| `0.5`                      | `f32`, `f64`                                   |
| `"text"`, or a bare `word` | `String`                                       |
| `true`, `false`            | `bool`                                         |
| Anything                   | `Value`                                        |

An integer is not converted to a float: a command that takes an `f32` has to be written `>> play("bell", 1.0)`, not `>> play("bell", 1)`.

A call that does not fit is logged and skipped, and the dialogue carries on. The log is worded for whoever wrote the line: `argument 1: expected float, got int`. A command the game never registered is logged the same way.

## Waiting for a command

Some commands take time: an animation, a fade, a pause. The dialogue waits for one when two things agree, the line says `await` and the command answers `CommandFlow::Blocking`.

```replique
:= start
Alice: Wait for it...
>> await pause(2.0)
Alice: Now!
---
```

The command takes the token of the call from the `DialogueCall` parameter, and keeps it for whatever will end the wait. Here a timer:

```rust
# use bevy::prelude::*;
# use bevy_replique::prelude::*;
#[derive(Component)]
struct Pause {
    timer: Timer,
    token: DialogueToken,
}

/// `>> await pause(2.0)`
#[replique_command]
fn pause(In(seconds): In<f32>, call: DialogueCall, mut commands: Commands) -> CommandFlow {
    commands.spawn(Pause {
        timer: Timer::from_seconds(seconds, TimerMode::Once),
        token: call.token(),
    });
    CommandFlow::Blocking
}

/// Resumes the dialogue of every pause that is over.
fn end_pauses(
    time: Res<Time>,
    mut pauses: Query<(Entity, &mut Pause)>,
    mut resume: MessageWriter<ResumeDialogue>,
    mut commands: Commands,
) {
    for (entity, mut waiting) in &mut pauses {
        if waiting.timer.tick(time.delta()).is_finished() {
            resume.write(ResumeDialogue {
                token: waiting.token,
                input: ResumeInput::Advance,
            });
            commands.entity(entity).despawn();
        }
    }
}

# let mut app = App::new();
app.add_dialogue_command(pause)
    .add_systems(Update, end_pauses);
```

Without the `await`, the dialogue does not wait even if the command answers `Blocking`: the line did not ask for it. The pause still happens, the dialogue simply carries on, and a warning is logged.

`call.runner()` gives the entity of the runner that called the command, for a command that acts on its own dialogue.

## Types of your own

`#[derive(RepliqueValue)]` lets a type of the game be written in a dialogue. An enum without data is written as a word, a struct with named fields as a dict:

```replique
:= start
>> face(Alice, left)
>> spawn({name: "Bob", hp: 12})
---
```

```rust
# use bevy::prelude::*;
# use bevy_replique::prelude::*;
#[derive(RepliqueValue, Debug)]
enum Direction {
    #[replique(alias = "left")]
    Left,
    #[replique(alias = "right")]
    Right,
}

#[derive(RepliqueValue, Debug)]
struct Stats {
    name: String,
    #[replique(alias = "hp")]
    health: i64,
}

/// `>> face(Alice, left)`
#[replique_command]
fn face(In((who, direction)): In<(String, Direction)>) {
    info!("{who} now faces {direction:?}");
}

/// `>> spawn({name: "Bob", hp: 12})`
#[replique_command]
fn spawn(In((stats,)): In<(Stats,)>) {
    info!("spawning {stats:?}");
}

# let mut app = App::new();
app.add_dialogue_command(face)
    .add_dialogue_command(spawn);
```

A variant is matched as it is spelled, `Left` here: `alias` gives it the spelling the dialogue uses. On a field, `alias` is the key of the dict.

A word the enum does not know, or a key the dict lacks, refuses the call with a message that names it: ``argument 1: `up` is not a `Direction` ``, ``argument 0: key `hp`: missing``.

A type of your own is taken inside a tuple, `(Stats,)` for a single argument.

## Commands with several shapes

`#[derive(RepliqueArgs)]` reads the whole call instead of one argument, for the calls a tuple cannot describe.

An enum picks its variant from the first word, so that one command covers several shapes:

```rust
# use bevy::prelude::*;
# use bevy_replique::prelude::*;
/// `>> camera(shake)` or `>> camera(move, 120.0, 40.0)`
#[derive(RepliqueArgs, Debug)]
enum CameraOrder {
    #[replique(alias = "shake")]
    Shake,
    #[replique(alias = "move")]
    MoveTo { x: f32, y: f32 },
}

#[replique_command]
fn camera(In(order): In<CameraOrder>) {
    info!("camera: {order:?}");
}
# let mut app = App::new();
# app.add_dialogue_command(camera);
```

A field marked `#[variadic]` collects every argument that is left:

```rust
# use bevy::prelude::*;
# use bevy_replique::prelude::*;
/// `>> add_scene(Alice)` or `>> add_scene(Alice, Bob, Caroline)`
#[derive(RepliqueArgs, Debug)]
struct Cast(#[variadic] Vec<String>);

#[replique_command]
fn add_scene(In(Cast(characters)): In<Cast>) {
    info!("on stage: {characters:?}");
}
# let mut app = App::new();
# app.add_dialogue_command(add_scene);
```

## Reading the commands yourself

Every command also goes out as a `DialogueCommand` message, with its `name`, its `args` and whether it is `awaited`. A game can read it to log or to observe what a dialogue does. The registered commands remain the way to act on them: a command with no system registered under its name is reported as an error.
