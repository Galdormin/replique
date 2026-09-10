# Replique

Replique is a pure Rust dialogue system, with first-class support for [Bevy](https://bevyengine.org).

It is inspired by [Yarn Spinner](https://www.yarnspinner.dev) and [Godot Dialogue Manager](https://github.com/nathanhoad/godot_dialogue_manager). Dialogues are written in plain text files that look like this:

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

| Syntax                   | Meaning                            |
| ------------------------ | ---------------------------------- |
| `:= name`                | Start a node named `name`          |
| `---`                    | End the current node               |
| `Speaker: text`          | A line of dialogue                 |
| `-> text`                | A choice offered to the player     |
| `>> command(arg1, arg2)` | A command to execute on the engine |
| `=> name`                | Jump to the node `name`            |

Indentation under a `->` line defines the lines played when that choice is picked.

## Crates

| Crate                                   | Description                                     | Docs                                                         |
| --------------------------------------- | ----------------------------------------------- | ------------------------------------------------------------ |
| [`replique`](crates/replique)           | The engine-agnostic parser and dialogue runtime | [doc.rs](https://docs.rs/replique/latest/replique)           |
| [`bevy_replique`](crates/bevy_replique) | Bevy plugin built on top of `replique`          | [doc.rs](https://docs.rs/bevy_replique/latest/bevy_replique) |
| [`replique-cli`](crates/replique-cli)   | Command line tool to check dialogue files       | [README](crates/replique-cli/README.MD)                      |
| [`replique-lsp`](crates/replique-lsp)   | Language server for editor support              | [README](crates/replique-lsp/README.MD)                      |

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
