---
summary: Creates a linear dimension parallel to the line between its two extension line origins.
related: DIMLINEAR, DIMCONTINUE, DIMBASELINE, DIMJOGLINE, DIMSTYLE, DIM
ribbon: Annotate tab › Dimensions panel
---

## Usage
1. Specify the first extension line origin, or press Enter to select an object.
2. Specify the second extension line origin. If you selected an object, its two ends are used instead.
3. Move the cursor to place the dimension line, or type an option.
4. Click to place the dimension. The command ends.

## Options
| Option | What it does |
|---|---|
| **MText** (`M`) | Asks for formatted dimension text on the command line. Leave it blank to use the measured value. |
| **Text** (`T`) | Asks for the dimension text. Leave it blank, or type `<>`, to use the measured value. |
| **Angle** (`A`) | Asks for the angle of the dimension text in degrees. |

## Notes
- The dimension measures the true distance between the two points, whatever their direction.
- You can select a line, an arc (its two ends), a circle (a diameter through your pick), or a polyline segment (the segment nearest your pick).
- A dimension made from a selected object is associated with it (see [`DIMASSOC`](DIMASSOC-System-Variable)).
- You can pick points through a layout viewport; the dimension then shows the model measurement.
