# Replique CLI

Command-line tools for the [Replique](https://github.com/Galdormin/replique) dialogue format. Installs a single binary, `replique`.

```sh
cargo install replique-cli
```

## `replique check`

Parses the given files and reports everything that is wrong with them.

```sh
replique check assets/dialogue/simple.rep
replique check --recursive assets/dialogue
```

```
error[jump-unknown-node]: jump to unknown node `nope`
  --> assets/dialogue/simple.rep:3:1
  |
3 | => nope
  | ^^^^^^^

1 error(s), 0 warning(s) in 1 file(s)
```

`--format short` prints one line per diagnostic, which is easier to grep and to
parse from an editor:

```
assets/dialogue/simple.rep:3:1: error[jump-unknown-node]: jump to unknown node `nope`
```

| Flag                               |                                             |
| ---------------------------------- | ------------------------------------------- |
| `-r`, `--recursive`                | Walk directories all the way down           |
| `-f`, `--format <pretty\|short>`   | Diagnostic layout, `pretty` by default      |
| `-W`, `--warnings-as-errors`       | Fail on warnings too                        |
| `-q`, `--quiet`                    | Print nothing, report through the exit code |
| `--schema <path>`                  | Also check against a `replique.toml`        |

Diagnostics and the summary go to stderr, like any other linter. Exit code is `0` when clean, `1` when a file has errors, `2` when the command
itself failed — an unreadable path, for instance.

### Checking against the schema of the project

On its own, `check` only knows the language. With `--schema`, it also knows the
project: the speakers, tags, functions and commands declared in its
`replique.toml`.

```toml
speakers = ["Robin", "Fanny", "Bertille"]

[tags]
happy = { scope = ["Robin", "Fanny"] }
sad = { scope = ["Fanny"] }
delay = {}

[functions]
get_flower = "1-2"

[commands]
add_scene = "1+"
change_mood = "2"
```

```sh
replique check --recursive --schema replique.toml assets/dialogue
```

```
warning[unknown-speaker]: speaker `Fany` is unknown
  --> assets/dialogue/simple.rep:2:1
  |
2 | Fany: Hi!
  | ^^^^

warning[wrong-arity]: `change_mood` expects 2 arguments, got 1
  --> assets/dialogue/simple.rep:3:4
  |
3 | >> change_mood(Fanny)
  |    ^^^^^^^^^^^^

0 error(s), 2 warning(s) in 1 file(s)
```

| Code               | Reported when                                                   |
| ------------------ | --------------------------------------------------------------- |
| `unknown-speaker`  | A line is said by someone who is not in `speakers`              |
| `unknown-tag`      | A tag is not in `[tags]`                                        |
| `tag-out-of-scope` | A scoped tag is on a line of another speaker, or on a choice    |
| `unknown-command`  | A command is not in `[commands]`                                |
| `unknown-function` | A function is neither a builtin nor in `[functions]`            |
| `wrong-arity`      | A command or a function is given the wrong number of arguments |

A section the schema leaves out is not checked. The format of the file is
described in the [README of the project](https://github.com/Galdormin/replique#project-schema).

All of these are warnings, so they do not change the exit code unless
`--warnings-as-errors` is given. A `replique.toml` that is missing or cannot be
read is a failure of the command itself, and exits with `2`.

The schema is only used when `--schema` names it: unlike `replique-lsp`, `check`
does not look for a `replique.toml` on its own.

## In CI

```yaml
- run: cargo install replique-cli
- run: replique check --recursive --warnings-as-errors --schema replique.toml assets/dialogue
```

## License

Dual-licensed under MIT or Apache-2.0, at your option.
