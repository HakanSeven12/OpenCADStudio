---
summary: Lines up several text objects along a line, with even, fixed or current spacing.
related: JUSTIFYTEXT, TEXT, MTEXT, SCALETEXT
ribbon: Annotate tab › Text panel
---

## Usage
1. The command line shows the current alignment and spacing mode.
2. Select the text objects to align, or type an option. Press Enter when done.
3. Select the text object to align to, or type `P` (Point) to pick a start point instead.
4. Pick the second point that sets the direction, or type `O` (Options) to change the spacing first.
5. The texts move along the line from the first point towards the second.

## Options
| Option | What it does |
|---|---|
| **alIgnment** (`I`) | The alignment point used on each text: **Left**, **Center**, **Right**, **TL**, **TC**, **TR**, **ML**, **MC**, **MR**, **BL**, **BC**, **BR**. |
| **Options** (`O`) | Choose the spacing mode (below). |
| **Point** (`P`) | Pick the first point of the line instead of a text to align to. |

Spacing modes:

| Mode | What it does |
|---|---|
| **Distribute** (`D`) | Spreads the texts evenly between the two points. |
| **Set spacing** (`S`) | Places the texts one after another with the gap you enter between them. |
| **current Vertical** (`V`) | Keeps each text's current vertical position. This is the default. |
| **current Horizontal** (`H`) | Keeps each text's current horizontal position. |

## Notes
- Works on single-line and multiline text.
- When you align to a text, that text stays where it is.
- Text on locked layers is not moved.
- The alignment and spacing settings are remembered for the next use.
