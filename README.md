# Replique

Replique is a pure Rust dialogue system, with first-class support for [Bevy](https://bevyengine.org).

It is inspired by [YarnSpinner](https://www.yarnspinner.dev), [YarnSpinner-Rust](https://github.com/YarnSpinnerTool/YarnSpinner-Rust) and [Godot Dialogue Manager](https://github.com/nathanhoad/godot_dialogue_manager). Dialogues are written in plain text files that look like this:

```
:= start
Alice: Hi! How are you?
Bob: Great, and you? Did you prepare the afternoon meeting?
Alice: Not at all, I was too tired yesterday. I'll get to it right away.
-> Good luck then!
    Alice: See you later!
-> I can help if you want.
    Alice: Thanks, I'd like that. Meet at the break?
    Bob: Perfect, see you!
=> meeting
---

:= meeting
Caroline: Hello everyone, now that we are all here we can start.
Caroline: Alice... You can start...
---
```

## Syntax at a glance

| Syntax                               | Meaning                                         |
| ------------------------------------ | ----------------------------------------------- |
| `:= name`                            | Start a node named `name`                       |
| `---`                                | End the current node                            |
| `Speaker: text`                      | A line of dialogue                              |
| `-> text`                            | A choice offered to the player                  |
| `>> command(arg1, arg2)`             | A command to execute on the engine              |
| `=> name`                            | Jump to the node `name`                         |
| `[let $var = value]`                 | Set a variable                                  |
| `[$var]`                             | Insert a value into the line                    |
| `[if cond]`, `[elif cond]`, `[else]` | Play a block only under a condition             |
| `[while cond]`                       | Repeat a block, with `[break]` and `[continue]` |

## Variables, conditions and loops

Variables hold numbers, strings, booleans and dictionaries. Anything written
between `[` and `]` inside a line is evaluated and inserted where it stands:

```
:= shop
[let $gold = 12]
[let $player = {name: "Alice", hp: 7}]

Merchant: Welcome [$player.name]! You have [$gold] coins.

[if $gold >= 10]
    Merchant: The lantern is yours.
    [let $gold = $gold - 10]
[elif $gold > 0]
    Merchant: Not quite enough, come back later.
[else]
    Merchant: Come back when you can pay.

[while $gold > 0]
    [let $gold = $gold - 1]
    >> ring_bell()

Merchant: [$player.name], you are leaving with [$gold] coins.
---
```

## Crates

| Crate                                   | Description                                     | Docs                                                         |
| --------------------------------------- | ----------------------------------------------- | ------------------------------------------------------------ |
| [`replique`](crates/replique)           | The engine-agnostic parser and dialogue runtime | [doc.rs](https://docs.rs/replique/latest/replique)           |
| [`bevy_replique`](crates/bevy_replique) | Bevy plugin built on top of `replique`          | [doc.rs](https://docs.rs/bevy_replique/latest/bevy_replique) |
| [`replique-cli`](crates/replique-cli)   | Command line tool to check dialogue files       | [README](crates/replique-cli/README.md)                      |
| [`replique-lsp`](crates/replique-lsp)   | Language server for editor support              | [README](crates/replique-lsp/README.md)                      |

## Versions

| Replique | Bevy |
| -------- | ---- |
| 0.1.0    | 0.19 |

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
