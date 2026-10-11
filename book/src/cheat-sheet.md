# Cheat sheet

## Statements

| Syntax                               | Meaning                                          |
| ------------------------------------ | ------------------------------------------------ |
| `:= name`                            | Start a node named `name`                        |
| `---`                                | End the current node                             |
| `=> name`                            | Jump to the node `name`                          |
| `<=> name`                           | Jump to the node `name`, and come back after it  |
| `=> RETURN`                          | Come back to the last `<=>`, or end the dialogue |
| `=> END`                             | End the dialogue                                 |
| `Speaker: text`                      | A line of dialogue                               |
| `text`                               | A line with no speaker                           |
| `-> text`                            | A choice offered to the player                   |
| `-> text [if cond]`                  | A choice only available under a condition        |
| `>> command(arg1, arg2)`             | Run a command of the game                        |
| `>> await command()`                 | Run a command and wait for it to be over         |
| `[let $var = value]`                 | Set a variable                                   |
| `[let $dict.key = value]`            | Set a key of a dict                              |
| `[if cond]`, `[elif cond]`, `[else]` | Play a block only under a condition              |
| `[while cond]`                       | Repeat a block, with `[break]` and `[continue]`  |
| `// comment`                         | A comment, up to the end of the line             |

Blocks are indented with four spaces.

## Inside a line or a choice

| Syntax                 | Meaning                                    |
| ---------------------- | ------------------------------------------ |
| `[$var]`, `[$a + 1]`   | Insert the value of an expression          |
| `#name`, `#name:value` | A tag, at the start or the end of the text |
| `\#word`               | A word that starts with a `#`              |

## Expressions

| Syntax                           | Meaning                                            |
| -------------------------------- | -------------------------------------------------- |
| `12`, `0.5`, `"text"`, `word`    | Integer, float, string, bare-word string           |
| `true`, `false`                  | Booleans                                           |
| `{name: "Alice", hp: 7}`         | A dict                                             |
| `$var`, `$dict.key`              | A variable, a key of a dict                        |
| `+`, `-`, `*`, `/`               | Arithmetic                                         |
| `==`, `!=`, `<`, `<=`, `>`, `>=` | Comparison                                         |
| `and`, `or`, `not`               | Logic, also written `&&`, `\|\|`, `!`              |
| `$dict has key`                  | Whether a dict holds a key                         |
| `Alice has Angry`                | Whether a subject has a feature, asked of the game |
| `name(arg1, arg2)`               | Call a function                                    |

Builtin functions: `upper`, `lower`, `capitalize`, `len`, `has`, `abs`, `min`, `max`, `round`, `floor`, `ceil`. See [Commands and functions](syntax/commands.md#builtins).
