# Variables and values

## Variables

`[let]` assigns a variable. A variable is written with a `$`, and exists from its first assignment until the end of the dialogue, whichever node set it.

```replique
:= start
[let $gold = 12]
[let $gold = $gold - 2]
Alice: I have [$gold] coins left.
---
```

Reading a variable that was never assigned stops the dialogue with an error.

## Values

| Type    | Written as                        |
| ------- | --------------------------------- |
| Integer | `12`, `-3`                        |
| Float   | `0.5`, `3.0`                      |
| String  | `"Alice"`, or a bare word `Alice` |
| Boolean | `true`, `false`                   |
| Dict    | `{name: "Alice", hp: 7}`          |

A string is written between double quotes. Inside it, `\"` is a quote and `\\` a backslash. A single word that is neither `true`, `false` nor an operator can be written without quotes: `Alice` and `"Alice"` are the same string.

## Dicts

A dict groups values under keys. Keys are bare words, and a value can be any expression, another dict included:

```replique
:= start
[let $player = {name: "Alice", hp: 7, stats: {strength: 3}}]
Alice: I am [$player.name], with [$player.stats.strength] in strength.
[let $player.hp = $player.hp - 4]
[let $player.stats.strength = 4]
---
```

A `.` reads the value under a key, and `[let]` writes one the same way. Reading a key the dict does not hold stops the dialogue with an error: use [`has`](expressions.md#has) to ask first.

`{}` is an empty dict.
