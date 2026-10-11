# Functions

A [function](../syntax/commands.md#functions) is how a dialogue _asks_ the game something. Like a command, it is a system; unlike a command, it gives a value back and the dialogue never waits for it.

```replique
:= shop
Merchant: Welcome! You have [gold()] coins.
[if gold() >= price_of("lantern")]
    Merchant: The lantern is yours.
[else]
    Merchant: Come back when you can pay.
---
```

```rust
# use bevy::prelude::*;
# use bevy::platform::collections::HashMap;
# use bevy_replique::prelude::*;
#[derive(Resource)]
struct Gold(i64);

#[derive(Resource, Default)]
struct Prices(HashMap<String, i64>);

/// `[gold()]`
#[replique_function]
fn gold(In(()): In<()>, purse: Res<Gold>) -> i64 {
    purse.0
}

/// `[price_of("lantern")]`
#[replique_function]
fn price_of(In(item): In<String>, prices: Res<Prices>) -> Result<i64, String> {
    prices
        .0
        .get(&item)
        .copied()
        .ok_or_else(|| format!("nothing called `{item}` is for sale"))
}

# let mut app = App::new();
# app.insert_resource(Gold(12)).init_resource::<Prices>();
app.add_dialogue_function(gold)
    .add_dialogue_function(price_of);
```

`#[replique_function]` and `add_dialogue_function` work as their command counterparts do, `name = "..."` included. The arguments are read the same way, see [Arguments](commands.md#arguments). [The name of the function becomes a type](commands.md#the-name-becomes-a-type) here too, which is why the resource above is not bound to a parameter called `gold`.

## What a function gives back

| Rust type                       | Value in the dialogue                                               |
| ------------------------------- | ------------------------------------------------------------------- |
| Integers                        | An integer                                                          |
| `f32`, `f64`                    | A float                                                             |
| `String`                        | A string                                                            |
| `bool`                          | A boolean                                                           |
| `HashMap<String, Value>`        | A dict                                                              |
| A type deriving `RepliqueValue` | A word or a dict, as it is [written](commands.md#types-of-your-own) |
| `Value`                         | Itself                                                              |

A struct that derives `RepliqueValue` comes back as a dict, which the dialogue reads key by key:

```replique
:= start
[let $stats = stats_of(Alice)]
[if $stats.hp > 10]
    Alice: I feel fine, with [$stats.hp] hit points.
---
```

```rust
# use bevy::prelude::*;
# use bevy_replique::prelude::*;
#[derive(Component)]
struct Character {
    name: String,
    hp: i64,
    gold: i64,
}

#[derive(RepliqueValue)]
struct Stats {
    hp: i64,
    gold: i64,
}

/// `[let $stats = stats_of(Alice)]`
#[replique_function]
fn stats_of(In(who): In<String>, characters: Query<&Character>) -> Result<Stats, String> {
    characters
        .iter()
        .find(|character| character.name == who)
        .map(|character| Stats {
            hp: character.hp,
            gold: character.gold,
        })
        .ok_or_else(|| format!("{who} is not on the scene"))
}
# let mut app = App::new();
# app.add_dialogue_function(stats_of);
```

## When there is no answer

A function that cannot answer returns a `Result`. On an `Err`, the dialogue stops where it is and the error is logged: there is no value to carry on with.

Calling a function the game never registered stops the dialogue the same way.

## A function only reads

A function can read anything a system can, but cannot change the world: registering one that takes a `ResMut`, a mutable query or `Commands` does not compile. A dialogue evaluates its expressions whenever it needs to, and a function that changed something would do it at times nobody chose. Changing the game is what a [command](commands.md) is for.

## The builtins

The [builtin functions](../syntax/commands.md#builtins), `upper`, `max` and the others, are always there and need no registration.
