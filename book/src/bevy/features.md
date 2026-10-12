# Subjects and features

[`has`](../syntax/expressions.md#has) asks the game a yes-or-no question without calling anything: `Alice has Angry` asks whether the entity named `Alice` holds the component named `Angry`.

```replique
:= start
[if Alice has Angry]
    Alice: Give it back!
[else]
    Alice: Could I have it back?
---
```

Neither name means anything to Bevy, so the game gives both.

A **subject** is an entity with a name, given by the `RepliqueSubject` component. A **feature** is a component with a name, given by `register_replique_feature`:

```rust
# use bevy::prelude::*;
# use bevy_replique::prelude::*;
#[derive(Component)]
struct Angry;

fn spawn_alice(mut commands: Commands) {
    commands.spawn((RepliqueSubject::new("Alice"), Angry));
}

# let mut app = App::new();
app.register_replique_feature::<Angry>("Angry")
    .add_systems(Startup, spawn_alice);
```

From there `Alice has Angry` is true exactly while that entity holds an `Angry`. Only the presence of the component is looked at, so a feature follows the game without any code: remove the component and the dialogue takes the other branch.

Names are matched as they are written, case included.

## Reading the value of a feature

A feature can also hold a value, read with a `.` after the name of the subject:

```replique
:= start
[if Alice has Mana and Alice.Mana >= 10.0]
    Alice: I have [Alice.Mana] mana, enough for a spell.
[else]
    Alice: I am out of mana.
---
```

Such a feature is registered with `register_replique_feature_value`, which asks the component to be `Clone` and `IntoValue`. `#[derive(RepliqueValue)]` already implement `IntoValue`:

```rust
# use bevy::prelude::*;
# use bevy_replique::prelude::*;
#[derive(Component, Clone, RepliqueValue)]
struct Mana(f64);

fn spawn_alice(mut commands: Commands) {
    commands.spawn((RepliqueSubject::new("Alice"), Mana(12.0)));
}

# let mut app = App::new();
app.register_replique_feature_value::<Mana>("Mana")
    .add_systems(Startup, spawn_alice);
```

The value is read off the component each time the dialogue asks, so it follows the game. A struct with named fields comes as a dict, read further with another `.`: `Alice.Stats.hp`.

Reading the feature of a subject that does not hold it stops the dialogue with an error.

## Unknown names

A subject or a feature the game was never given is a mistake in the dialogue, not a `false`: it stops the dialogue with an error. A typo therefore does not quietly send the player down the `[else]`.

## The same name in two dialogues

A subject is visible to every dialogue. `only_for` restricts it to the dialogue of one runner, which lets two dialogues running side by side each have their own `Guard`:

```rust
# use bevy::prelude::*;
# use bevy_replique::prelude::*;
# #[derive(Component)]
# struct Angry;
fn spawn_guards(mut commands: Commands, assets: Res<AssetServer>) {
    let dialogue: Handle<RepliqueDialogue> = assets.load("dialogue/gate.rep");
    let north_gate = commands.spawn(DialogueRunner::new(dialogue.clone())).id();
    let south_gate = commands.spawn(DialogueRunner::new(dialogue)).id();

    commands.spawn((RepliqueSubject::new("Guard").only_for(north_gate), Angry));
    commands.spawn(RepliqueSubject::new("Guard").only_for(south_gate));
}
# let mut app = App::new();
# app.add_systems(Startup, spawn_guards);
```

In the dialogue of the north gate, `Guard has Angry` is true; in the one of the south gate, it is false.

A subject restricted to the runner that asks comes before one visible to all, so a dialogue can also have its own `Alice` in place of the usual one. Two subjects of the same name at the same level are ambiguous, and asking about them is an error.
