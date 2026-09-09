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

| Flag                             |                                             |
| -------------------------------- | ------------------------------------------- |
| `-r`, `--recursive`              | Walk directories all the way down           |
| `-f`, `--format <pretty\|short>` | Diagnostic layout, `pretty` by default      |
| `-W`, `--warnings-as-errors`     | Fail on warnings too                        |
| `-q`, `--quiet`                  | Print nothing, report through the exit code |

Diagnostics and the summary go to stderr, like any other linter. Exit code is `0` when clean, `1` when a file has errors, `2` when the command
itself failed — an unreadable path, for instance.

## In CI

```yaml
- run: cargo install replique-cli
- run: replique check --recursive --warnings-as-errors assets/dialogue
```

## License

Dual-licensed under MIT or Apache-2.0, at your option.
