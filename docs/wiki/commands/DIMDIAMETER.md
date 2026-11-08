---
summary: Creates a diameter dimension for a circle or an arc.
related: DIMRADIUS, DIMJOGGED, DIMCENTER, DIMSTYLE, DIM
ribbon: Annotate tab › Dimensions panel
---

## Usage
1. Select an arc or a circle.
2. Move the cursor to place the dimension line, or type an option. The line runs across the circle through the cursor.
3. Click to place the dimension. The command ends.

## Options
| Option | What it does |
|---|---|
| **MText** (`M`) | Opens the text editor so you can replace or add to the measured text. |
| **Text** (`T`) | Asks for the dimension text. Leave it blank, or type `<>`, to use the measured value. |
| **Angle** (`A`) | Asks for the angle of the dimension text in degrees. |

## Notes
- The measured value gets the diameter symbol.
- The dimension is associated with the picked object (see [`DIMASSOC`](DIMASSOC-System-Variable)).
- The dimension uses the current dimension style.
