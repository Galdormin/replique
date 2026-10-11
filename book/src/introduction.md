# Introduction

Replique is a dialogue system written in Rust, with first-class support for [Bevy](https://bevyengine.org). Dialogues are written in plain text files, usually with the `.rep` extension, that look like this:

```replique
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

This book describes the language those files are written in, then how to play them in a Bevy game.

## Where to go from here

The chapters of _Writing dialogues_ go through the language one piece at a time, and the [cheat sheet](cheat-sheet.md) holds all of it on one page. They are meant for whoever writes the dialogues, and need no Rust.

_Using Replique with Bevy_ is for whoever wires those dialogues into a game: loading a file, showing its lines and choices, and giving it commands and functions of the game to call.

The API of each crate, and the tools around the format, are documented with the crates:

| Crate                                                                                 | What it is                                   |
| ------------------------------------------------------------------------------------- | -------------------------------------------- |
| [`replique`](https://docs.rs/replique)                                                | The engine-agnostic parser and runtime       |
| [`bevy_replique`](https://docs.rs/bevy_replique)                                      | The Bevy plugin built on top of `replique`   |
| [`replique-cli`](https://github.com/Galdormin/replique/tree/main/crates/replique-cli) | `replique check`, to lint dialogue files     |
| [`replique-lsp`](https://github.com/Galdormin/replique/tree/main/crates/replique-lsp) | Language server, for diagnostics as you type |
