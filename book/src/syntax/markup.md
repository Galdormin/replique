# Markup

A markup gives information on how a piece of text is shown: in bold, with a wave, after a pause. It is written between `<` and `>` in the text of a line or of a choice.

```replique
:= start
Alice: Have you heard? The <b>king</b> is coming <i>tonight</i>.
Bob: <wave>Tonight?</wave><pause=0.5/> But the streets are so dirty!
---
```

The player reads _Have you heard? The king is coming tonight._ The game receives the markups apart from the text, each one with the part of the text it covers, and decides what they mean. Replique gives no meaning to any name: `<b>` is bold only if the game draws it so.

## Opening and closing

`<name>` opens a markup, and `</name>` closes it. What is written between the two is what it covers, inline expressions included:

```replique
:= start
[let $name = "Bob"]
Alice: Welcome, <b>[$name]</b>!
---
```

Markups nest, and close in the reverse order they open: `<b><i>both</i></b>`. A markup opens and closes on the same line.

## Standing on its own

`<name/>` covers no text: it stands at a position of the text, for something that happens there rather than over a part of it, such as a pause or a sound.

```replique
:= start
Alice: Well...<pause=1/> I suppose so.
---
```

## Values and attributes

A markup can take a value right after its name, and attributes after it:

```replique
:= start
Alice: <color=red>Careful</color>, the <wave speed=10 amplitude=2>water</wave> is deep.
Alice: Meet me at <link="the old mill">the mill</link>.
---
```

A value is a word, or a text between double quotes when it holds blanks. Inside the quotes, `\"` writes a quote. Values are kept as text, whatever they look like: `0.5` reaches the game as the text `0.5`.

## Writing a `<`

A `<` only opens a markup when a letter, a `_` or a `/` comes right after it, so `I <3 you` and `a < b` are plain text. To write a word that starts with a `<`, put a `\` in front of it:

```replique
:= start
Alice: I <3 you, and \<b> is not bold.
---
```

The value of an expression is never read as markup: if `$name` holds `<b>`, the player reads `<b>`.
