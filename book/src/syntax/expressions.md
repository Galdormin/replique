# Expressions

An expression is anything that gives a value: the right-hand side of a `[let]`, a condition, an argument, or what stands between `[` and `]` in the middle of a line.

## Operators

From the one that binds the loosest to the one that binds the tightest:

| Operators                               | Meaning                          |
| --------------------------------------- | -------------------------------- |
| `or`, `\|\|`                            | Either side is true              |
| `and`, `&&`                             | Both sides are true              |
| `==`, `!=`, `<`, `<=`, `>`, `>=`, `has` | Comparison, and `has`            |
| `+`, `-`                                | Addition, subtraction            |
| `*`, `/`                                | Multiplication, division         |
| `not`, `!`, `-`                         | Negation, of a boolean or a number |

Parentheses group: `($a + 1) * 2`.

```replique
:= start
[let $gold = 12]
[let $rich = $gold > 10 and not ($gold == 13)]
Alice: Half of it is [$gold / 2].
---
```

Arithmetic works on numbers. An operation between two integers gives an integer, except a division, which always gives a float: `7 / 2` is `3.5`. Dividing by zero stops the dialogue with an error.

`==` and `!=` compare two values of the same type, dicts included.

The types are checked when the file is read wherever they are known: `"a" + 1` or `[if 3]` is reported before the dialogue ever runs. What depends on a variable or on a function is checked when it is evaluated.

## `has`

`has` asks whether something holds something else. What it means depends on its left-hand side.

**On a dict**, it tells whether the dict holds a key, whatever the value under it. The key is a bare word, a string or a variable:

```replique
:= start
[let $stats = {strength: 12, agility: 0}]
[if $stats has strength]
    Alice: I can lift that.
[if not ($stats has magic)]
    Alice: No spell from me, though.
[let $key = "agility"]
[if $stats has $key]
    Alice: An agility of zero is still an agility.
---
```

Only the dict itself is searched, not the ones nested in it.

**On a string**, the question goes to the game: `Alice has Angry` asks it whether the subject `Alice` has the feature `Angry`. What a subject and a feature are is up to the game.

```replique
:= start
[if Alice has Angry]
    Alice: Give it back!
[else]
    Alice: Could I have it back?
---
```

`not` binds tighter than `has`: write `not ($stats has magic)`, with the parentheses.

## Functions

A function is called by its name followed by its arguments between parentheses, the `(` touching the name: `max($a, $b)`. See [Commands and functions](commands.md#functions).
