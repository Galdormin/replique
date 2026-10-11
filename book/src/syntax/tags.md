# Tags

A tag is what a line says about itself rather than to the player: a mood, a sound to play, an id. It is written `#name` or `#name:value`, and reaches the game apart from the text.

```replique
:= start
Alice: #angry Give it back! #sound:alice_01
---
```

Here the player reads _Give it back!_, and the game receives the tags `angry` and `sound`, the second one with the value `alice_01`.

A name is made of letters, digits and `_`, and starts with a letter or `_`. A value can also hold `.` and `-`, and start with a digit: `#volume:0.5`, `#voice:alice-01.ogg`.

## Where a tag goes

Tags are written at the **start** or at the **end** of the text, as many as needed. A tag in the middle of a sentence is not read: it stays in the text, with a warning. Giving the same name twice on a line is a warning too.

A line can be made of tags alone:

```replique
:= start
Alice: #silence
---
```

## On a choice

A choice takes tags the same way. When it has a [condition](choices.md#conditional-choices), the tags come before it:

```replique
:= start
[let $gold = 3]
-> #hostile Attack
-> Bribe the guard #cost:3 [if $gold >= 3]
---
```

## Writing a `#`

A `#` only opens a tag after a blank and before a letter, so `C#` and `#1` are plain text. To write a word that starts with a `#`, put a `\` in front of it:

```replique
:= start
Agent #2: We are #1 in C#.
Alice: \#angry is trending.
---
```
