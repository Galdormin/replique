# Markup

The [markups](../syntax/markup.md) of a line or of a choice reach the game apart from the text, in the `markups` field of `DialogueLine` and of `DialogueChoice`. Each `Markup` has its `name`, its `value` and its `attrs` as they are written, and the `range` of the text it covers, in bytes: `&line.text[markup.range.clone()]` is the piece of text under it.

What a markup means is up to the game. `bevy_replique` styles nothing on its own.

## Cutting the text

Drawing a line with its markups means cutting the text wherever a markup starts or ends, and styling each run after the markups it lies under. `pieces()` does the cutting:

```replique
:= start
Alice: <b>Hi</b> there<pause=0.5/>, <b><i>Bob</i></b>.
---
```

```rust
# use bevy::prelude::*;
# use bevy_replique::prelude::*;
fn read_pieces(mut lines: MessageReader<DialogueLine>) {
    for line in lines.read() {
        for piece in line.pieces() {
            match piece {
                // "Hi" under `b`, " there", ", ", "Bob" under `b` and `i`, "."
                Piece::Text(segment) => {
                    info!("{:?} bold: {}", segment.text, segment.has("b"));
                }
                // `<pause=0.5/>`, between " there" and ", "
                Piece::Marker(marker) => {
                    info!("marker {} at {}", marker.name, marker.range.start);
                }
            }
        }
    }
}
# let mut app = App::new();
# app.add_systems(Update, read_pieces);
```

A `Piece::Text` holds a `Segment`: a run of `text` with the same markups over all of it, listed in `markups` from the outermost to the innermost. Writing the segments one after the other gives the whole text back.

`segment.has(name)` tells whether the run is under a markup of that name, and `segment.get(name)` gives the innermost one, to read its value or its attributes:

```rust
# use bevy::prelude::*;
# use bevy_replique::prelude::*;
/// The colour of `<color=red>`, if the run is under one.
fn color_of(segment: &Segment) -> Option<Color> {
    match segment.get("color")?.value.as_deref()? {
        "red" => Some(Color::srgb(0.9, 0.3, 0.3)),
        "blue" => Some(Color::srgb(0.3, 0.5, 0.9)),
        _ => None,
    }
}
```

A `Piece::Marker` is a markup that covers no text, such as `<pause/>`, at the position it stands in the text. It comes before the run that starts at that position.

`DialogueChoice` has the same `pieces()`, for the text of a choice.

## Drawing it in Bevy

Each segment can become a `TextSpan` of its own, under the `Text` of the line. Here in bold under a `<b>` and in italic under an `<i>`:

```rust
# use bevy::{prelude::*, text::{FontSource, FontStyle, FontWeight, GenericFontFamily}};
# use bevy_replique::prelude::*;
/// The text node the line is written in.
#[derive(Component)]
struct LineText;

fn show_line(
    mut commands: Commands,
    mut lines: MessageReader<DialogueLine>,
    root: Single<Entity, With<LineText>>,
) {
    for line in lines.read() {
        commands.entity(*root).despawn_children();

        for piece in line.pieces() {
            // Markers are left to a typewriter, which this one does not have.
            let Piece::Text(segment) = piece else {
                continue;
            };

            let font = TextFont {
                font: FontSource::Generic(GenericFontFamily::SansSerif),
                weight: if segment.has("b") { FontWeight::BOLD } else { FontWeight::NORMAL },
                style: if segment.has("i") { FontStyle::Italic } else { FontStyle::Normal },
                ..default()
            };
            commands
                .entity(*root)
                .with_child((TextSpan::new(segment.text), font));
        }
    }
}
# let mut app = App::new();
# app.add_systems(Update, show_line);
```

The font Bevy embeds has no bold nor italic face, so asking for `FontWeight::BOLD` with it changes nothing on screen. The example above asks for the sans-serif font of the system instead, which needs the `system_font_discovery` feature of Bevy. Loading a font of your own with its bold and italic faces works as well.

The `bevy_markup` example draws a whole dialogue this way:

```sh
cargo run -p bevy_replique --example bevy_markup
```

## Markers and timing

A marker such as `<pause/>` only means something once the text is shown over time, letter by letter. `bevy_replique` has no typewriter yet, so it leaves markers to the game: walk the pieces in order, write each run as it goes, and act on a marker when it is reached.
