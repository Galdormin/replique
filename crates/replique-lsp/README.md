# Replique LSP

Language server for the [Replique](https://github.com/Galdormin/replique) dialogue format. Installs a single binary, `replique-lsp`, which speaks LSP over stdio.

```sh
cargo install replique-lsp
```

It parses with the same crate as `replique check`, so what the editor underlines
is what CI will fail on.

## What it does

### Diagnostics

Every `.rep` file is re-parsed on open and on each change, and the whole report is published: errors and warnings, each with its code.

### Project schema

When a `replique.toml` lies above a file, the file is also checked against it: speakers, tags, commands and functions the project does not declare are reported, the same way `replique check --schema` does.

The nearest `replique.toml` is used, looking up from the directory of the file and no further than the workspace folder. It is read again when it changes on disk, which shows at the next edit of a dialogue. A `replique.toml` that cannot be read is told in a message, and checks nothing.

### Completion

What is offered depends on where the cursor sits on the line:

| Where      | What                                     | Inserted as             |
| ---------- | ---------------------------------------- | ----------------------- |
| First word | Speakers seen in the file, then commands | `Alice: `, `>> shake()` |
| After `>>` | Commands called anywhere in the file     | `shake()`               |
| After `=>` | Nodes declared in the file               | `meeting`               |
| After `$`  | Variables assigned in the file           | `gold`                  |
| After `#`  | Tags written in the file                 | `angry`                 |

With a `replique.toml` above the file, the speakers, commands and tags it declares are offered instead of the ones seen in the file, and a tag kept for some speakers is only offered on their lines.

Names are collected from the whole file, nested blocks included: a speaker who only talks inside a choice, or a command only called in an `[if]` branch, is offered like any other.

### Not yet

Hover, go-to-definition, rename, document symbols, formatting and semantic tokens are not implemented.

## Editors

| Editor                   | Status                                                        |
| ------------------------ | ------------------------------------------------------------- |
| [Zed](../../editors/zed) | Extension in this repo, not published to the Zed registry yet |
| Anything else            | Works by pointing an LSP client at the `replique-lsp` binary  |

## License

Dual-licensed under MIT or Apache-2.0, at your option.
