# Nodes and jumps

## Nodes

A file is a list of **nodes**. A node is a named piece of dialogue: it starts with `:=` followed by its name, and ends with `---`.

```replique
:= start
Alice: Hi!
---

:= shop
Merchant: Welcome.
---
```

A name is made of letters, digits and `_`, and does not start with a digit. Two nodes of a file cannot share a name, and `END` is reserved.

Neither `:=` nor `---` is indented, and nothing but blank lines and comments is written outside of a node.

## Jumps

`=>` leaves the current node for another one. The target can be declared further down the file.

```replique
:= start
Alice: Follow me.
=> shop
---

:= shop
Merchant: Welcome.
---
```

A node that reaches its `---` without jumping ends the dialogue. `=> END` ends it from anywhere, which is handy inside a choice or a condition:

```replique
:= start
Alice: Are you coming?
-> Yes
    Alice: Let's go.
-> No
    Alice: Too bad.
    => END
Alice: It is not far.
---
```

## Indentation

Blocks are nested with indentation: what belongs to a choice, an `[if]` or a `[while]` is written one level deeper than it. A level is **four spaces**. Tabs are refused.

## Comments

`//` starts a comment that runs to the end of the line. A comment can stand on a line of its own or follow a statement.

```replique
:= start
// Alice is the first to speak.
Alice: Hi! // said with a smile
---
```
