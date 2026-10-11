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

A name is made of letters, digits and `_`, and does not start with a digit. Two nodes of a file cannot share a name, and `END` and `RETURN` are reserved.

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

## Jumping and coming back

`<=>` jumps to another node like `=>` does, but comes back once that node is over: the dialogue carries on with the line that follows the `<=>`.

```replique
:= start
Alice: Let me ask the merchant.
<=> haggle
Alice: Right, where were we?
---

:= haggle
Merchant: Ten coins.
Alice: Five.
Merchant: Eight, and that is my last word.
---
```

Here Alice says _Right, where were we?_ after the merchant's last line. This is the way to share a piece of dialogue between several places: each of them reaches it with a `<=>`, and gets the dialogue back afterwards.

The node that is reached does not need to know how it was reached. Its `---` comes back when a `<=>` led to it, and ends the dialogue otherwise.

`<=>` nest: a node reached with a `<=>` can use one itself, and each comes back to its own place. A plain `=>` inside such a node moves on to another node without coming back to the one it left, and the `<=>` still waits: the dialogue comes back when that other node is over.

### `=> RETURN` and `=> END`

`=> RETURN` does what the `---` of the node does, from anywhere in it: it comes back to the last `<=>`, or ends the dialogue when no `<=>` led here.

`=> END` ends the whole dialogue, whatever `<=>` are waiting.

```replique
:= start
Alice: Let me ask the merchant.
<=> haggle
Alice: Right, where were we?
---

:= haggle
Merchant: Ten coins.
-> Never mind
    => RETURN
-> I am leaving
    Merchant: Suit yourself.
    => END
-> Five
    Merchant: Eight, and that is my last word.
Merchant: A pleasure doing business.
---
```

With _Never mind_, the dialogue comes back to Alice right away. With _I am leaving_, it ends there and Alice says nothing more. With _Five_, the merchant finishes and the `---` comes back to Alice.

Neither `END` nor `RETURN` is a node, so `<=> END` and `<=> RETURN` are refused.

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
