# Choices

`->` offers a choice to the player. The `->` that follow each other at the same indentation form one group, shown together:

```replique
:= start
Alice: Are you coming to the meeting?
-> Yes
    Bob: With pleasure.
-> No
    Bob: Another time.
Alice: Fine.
---
```

What is indented under a choice is played when the player picks it. The dialogue then carries on after the group: here Alice says *Fine.* whatever the answer. A choice with nothing under it goes straight there.

Choices nest: a choice written under another one opens a new group, offered once the first one is picked.

```replique
:= start
Alice: Something to drink?
-> Yes
    Alice: Tea or coffee?
    -> Tea
        Alice: Right away.
    -> Coffee
        Alice: I am out of coffee.
-> No
    Alice: As you wish.
---
```

The text of a choice takes inline expressions like a line does, and can carry [tags](tags.md):

```replique
:= start
[let $price = 10]
-> Pay [$price] coins
-> Leave
---
```

## Conditional choices

A choice that ends with `[if <condition>]` is only **available** when the condition holds:

```replique
:= start
[let $is_sick = false]
Alice: How are you?
-> Fine
-> Getting by
-> I am sick [if $is_sick]
---
```

A choice that is not available is still handed to the game with the others, marked as such: it is up to the game to hide it or to show it greyed out. It cannot be picked either way.

The condition is the last thing on the line. Tags, if any, come before it:

```replique
:= start
[let $gold = 3]
-> #hostile Attack
-> Bribe the guard #cost:3 [if $gold >= 3]
---
```

At least one choice of a group has to be available when the group is reached: a group nobody can answer stops the dialogue with an error. Keep one choice without a condition when in doubt.
