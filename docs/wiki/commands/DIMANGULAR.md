---
summary: Measures the angle between two lines, the angle of an arc, an angle on a circle, or an angle given by three points.
related: DIMARC, DIMBASELINE, DIMCONTINUE, DIMSTYLE, DIM
ribbon: Annotate tab › Dimensions panel
---

## Usage
1. Select an arc, a circle or a line, or click an empty spot to use it as the angle vertex.
   - **Arc**: the angle the arc spans is measured.
   - **Circle**: your pick point is the first end of the angle. Specify the second extension line point.
   - **Line**: select a second line that is not parallel to the first.
   - **Vertex**: specify the first extension line point, then the second.
2. Move the cursor to place the dimension arc, or type an option. The side of the vertex you move to decides which angle is measured.
3. Click to place the dimension. The command ends.

## Options
| Option | What it does |
|---|---|
| **MText** (`M`) | Opens the text editor so you can replace or add to the measured text. |
| **Text** (`T`) | Asks for the dimension text. Leave it blank, or type `<>`, to use the measured value. |
| **Angle** (`A`) | Sets the angle of the dimension text. Type an angle or pick two points. |
| **Quadrant** (`Q`) | Pick a point to lock the measured angle to that quadrant, so moving the cursor no longer changes which angle is measured. |

## Notes
- Press Enter at the first prompt to switch between selecting an object and specifying a vertex.
- The objects must lie in the current working plane.
- The dimension is associated with the picked objects (see [`DIMASSOC`](DIMASSOC-System-Variable)).
