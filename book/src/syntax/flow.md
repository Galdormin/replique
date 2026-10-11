# Conditions and loops

## `[if]`, `[elif]`, `[else]`

`[if]` plays the block indented under it only when its condition is true. `[elif]` gives another condition to try when the ones before it failed, and `[else]` is played when none of them held.

```replique
:= shop
[let $gold = 12]
[if $gold >= 10]
    Merchant: The lantern is yours.
    [let $gold = $gold - 10]
[elif $gold > 0]
    Merchant: Not quite enough, come back later.
[else]
    Merchant: Come back when you can pay.
Merchant: Goodbye.
---
```

`[elif]` and `[else]` are written at the indentation of the `[if]` they belong to, and `[else]` comes last.

A condition has to be a boolean: `[if $gold]` is an error when `$gold` is a number, write `[if $gold > 0]`.

## `[while]`

`[while]` repeats its block as long as its condition is true.

```replique
:= start
[let $gold = 0]
[while $gold < 3]
    [let $gold = $gold + 1]
    Alice: I have [$gold] coins.
Alice: That will do.
---
```

Inside the block, `[break]` leaves the loop and `[continue]` goes straight to the next turn. Both act on the innermost `[while]`, and can be nested in a condition or in a choice:

```replique
:= start
[let $turn = 0]
[while true]
    [let $turn = $turn + 1]
    Alice: Turn [$turn]. One more?
    -> Yes
        [continue]
    -> No
        [break]
Alice: Done.
---
```

A dialogue that runs for too long without anything to show is stopped with an error, so a loop that never ends cannot freeze the game.
