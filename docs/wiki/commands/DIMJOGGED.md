---
summary: Creates a jogged radius dimension, for arcs and circles whose center is far away or off the sheet.
related: DIMRADIUS, DIMDIAMETER, DIMSTYLE, DIM
ribbon: Annotate tab › Dimensions panel
---

## Usage
1. Select an arc or a circle.
2. Specify the center location override: the point the dimension line starts from instead of the true center.
3. Move the cursor to place the dimension line, or type an option, then click.
4. Specify the jog location. The command ends.

## Options
| Option | What it does |
|---|---|
| **MText** (`M`) | Opens the text editor so you can replace or add to the measured text. |
| **Text** (`T`) | Asks for the dimension text. Leave it blank, or type `<>`, to use the measured value. |
| **Angle** (`A`) | Asks for the angle of the dimension text in degrees. |

## Notes
- The dimension still shows the true radius.
- The dimension uses the current dimension style.
