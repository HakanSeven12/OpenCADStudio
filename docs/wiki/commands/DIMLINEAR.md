---
summary: Creates a horizontal, vertical or rotated linear dimension.
related: DIMALIGNED, DIMCONTINUE, DIMBASELINE, DIMJOGLINE, DIMSTYLE, DIM
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
| **Angle** (`A`) | Asks for the angle of the dimension text in degrees. Leave it blank to use the default angle. |
| **Horizontal** (`H`) | Measures the horizontal distance only. |
| **Vertical** (`V`) | Measures the vertical distance only. |
| **Rotated** (`R`) | Measures along an angle you type, or that you show with two points. |

## Notes
- Without Horizontal, Vertical or Rotated, the cursor position decides whether the dimension is horizontal or vertical.
- You can select a line, an arc (its two ends), a circle (a diameter through your pick), or a polyline segment (the segment nearest your pick).
- A dimension made from a selected object is associated with it, so it follows when the object changes (see [`DIMASSOC`](DIMASSOC-System-Variable)).
- You can pick points through a layout viewport; the dimension then shows the model measurement.
- The dimension uses the current dimension style.
