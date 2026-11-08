---
summary: Creates a radius dimension for a circle or an arc.
related: DIMDIAMETER, DIMJOGGED, DIMSTYLE, DIMTEDIT
ribbon: Annotate tab › Dimensions panel
---

## Usage
1. Select an arc or a circle.
2. Move the cursor to place the dimension line, or type an option. The line runs from the center through the cursor.
3. Click to place the dimension. The command ends.

## Options
| Option | What it does |
|---|---|
| **MText** (`M`) | Opens the text editor so you can replace or add to the measured text. `<>` stands for the measured value. |
| **Text** (`T`) | Asks for the dimension text on the command line. Leave it blank to use the measured value. |
| **Angle** (`A`) | Asks for the angle of the dimension text in degrees. |

## Notes
- The dimension uses the current dimension style.
- Arc segments of a polyline and curved edges of a region can also be dimensioned; the segment nearest to your pick is used.
- The dimension is associated with the picked object, so it follows when the object changes (see [`DIMASSOC`](DIMASSOC-System-Variable)).
- You can pick an object through a layout viewport; the dimension then shows the model measurement.
