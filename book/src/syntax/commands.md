# Commands and functions

A dialogue reaches into the game in two ways: a command _does_ something, a function _answers_ something.

## Commands

`>>` runs a command of the game: play a sound, move a character, set a flag.

```replique
:= start
>> fade_in()
Alice: Do you hear that?
>> play("bell", 0.5, true)
>> set_flag(heard)
---
```

A command is a name followed by its arguments between parentheses. The parentheses are required even when there is no argument, and the `(` touches the name. Each argument is an [expression](expressions.md).

Which commands exist, and what they take, is defined by the game.

### `await`

`await` asks the game to wait until the command is over before the dialogue moves on. Without it, the game is free to carry on right away:

```replique
:= start
>> await play_anim("wave")
Alice: Hello!
>> await fade_out()
---
```

## Functions

A function gives a value back, and is called inside an expression:

```replique
:= start
[if gold() >= 10]
    Merchant: You have [gold()] coins, the lantern is yours.
---
```

A function never makes the dialogue wait. Like commands, the functions a dialogue can call are defined by the game, on top of the builtins below.

## Builtins

These functions are available to every dialogue:

| Function                       | Gives                                                                           |
| ------------------------------ | ------------------------------------------------------------------------------- |
| `upper(s)`                     | The string in upper case: `upper("bell")` is `"BELL"`                           |
| `lower(s)`                     | The string in lower case: `lower("Bell")` is `"bell"`                           |
| `capitalize(s)`                | The string with a capital first letter                                          |
| `len(x)`                       | The number of characters of a string, or of keys of a dict                      |
| `has(dict, key)`               | Whether the dict holds the key, as the [`has`](expressions.md#has) operator     |
| `abs(n)`                       | The absolute value, of the type it is given: `abs(-3)` is `3`                   |
| `min(a, b, ...)`               | The smallest of two numbers or more, all of one type                            |
| `max(a, b, ...)`               | The largest of two numbers or more, all of one type                             |
| `round(f)`, `round(f, digits)` | The float rounded, to `digits` decimals when given: `round(3.551, 2)` is `3.55` |
| `floor(f)`                     | The largest whole float under it: `floor(3.7)` is `3.0`                         |
| `ceil(f)`                      | The smallest whole float over it: `ceil(3.2)` is `4.0`                          |

```replique
:= start
[let $name = "alice"]
[let $hp = 7]
Narrator: [capitalize($name)] has [max($hp, 0)] hit points.
---
```
