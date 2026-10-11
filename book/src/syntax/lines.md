# Lines

A line is what a character says. It is written as the name of the speaker, a `:`, and the text:

```replique
:= start
Alice: Hi! How are you?
Bob: Great, and you?
---
```

The speaker is whatever comes before the first `:` of the line, so a name can hold spaces: `Old man: Who goes there?`.

## Lines without a speaker

A line with no `:` has no speaker. It suits a narrator, or a description:

```replique
:= start
The door creaks open.
Alice: Who is there?
---
```

Since whatever comes before the first `:` is read as the speaker, a line without a speaker cannot hold a `:` in its text.

## Inserting a value

An expression between `[` and `]` is evaluated when the line is reached, and replaced by its value:

```replique
:= start
[let $name = "Alice"]
[let $gold = 3]
Bob: Hello [$name], you owe me [$gold * 100] cents.
---
```

Anything described in [Expressions](expressions.md) can stand between the brackets. A line can also carry [tags](tags.md).
