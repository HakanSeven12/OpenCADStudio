---
summary: Measures the length along an arc or a polyline arc segment.
related: DIMANGULAR, DIMRADIUS, DIMSTYLE, DIM
ribbon: Annotate tab › Dimensions panel
---

## Usage
1. Select an arc or a polyline arc segment.
2. Move the cursor to place the dimension, or type an option.
3. Click to place the dimension. The command ends.

## Options
| Option | What it does |
|---|---|
| **MText** (`M`) | Opens the text editor so you can replace or add to the measured text. |
| **Text** (`T`) | Asks for the dimension text. Leave it blank, or type `<>`, to use the measured value. |
| **Angle** (`A`) | Asks for the angle of the dimension text in degrees. |
| **Partial** (`P`) | Measures only part of the arc. Specify the first and second points on the arc. |
| **Leader** (`L`) / **No Leader** (`N`) | Adds or removes a leader from the text to the arc. Offered only for arcs that span more than 90 degrees. |

## Notes
- Curved edges of a region can also be dimensioned.
- The dimension is associated with the picked object (see [`DIMASSOC`](DIMASSOC-System-Variable)).
