---
summary: Fills enclosed areas or selected closed objects with a hatch pattern, a solid fill or a gradient.
related: GRADIENT, HATCHEDIT, BOUNDARY, HATCHTOBACK, HPNAME
ribbon: Draw tab › Draw panel
---

## Usage
1. Pick internal point or [Select objects/Draw/Undo/seTtings]: click inside each area you want to fill. The boundary is found from the objects around the point.
2. Pick more areas, or type an option.
3. Press Enter to accept and create the hatch.

The Hatch settings panel opens at the top left of the drawing area while the command runs. Use it to choose the pattern, angle, scale, colors and the other settings below.

## Options
| Option | What it does |
|---|---|
| **Select objects** (`S`) | Pick closed objects to fill instead of clicking inside areas. |
| **picK internal point** (`K`) | Go back to clicking inside areas. |
| **Draw** (`D`) | Draw the boundary yourself, point by point (see below). |
| **Undo** (`U`) | Removes the last area you added. |
| **seTtings** (`T`) | Shows the Hatch settings panel. |
| **Accept** (Enter) | Creates the hatch from the areas picked so far. |

### Drawing the boundary
After **Draw**, the prompt is "Specify start point or [picK internal point/Select objects/Rectangle/Circle/Mode/seTtings]".

| Option | What it does |
|---|---|
| **Rectangle** (`R`) | Draws a rectangular boundary, with the usual rectangle prompts. |
| **Circle** (`C`) | Draws a circular boundary, with the usual circle prompts. |
| **Mode** (`M`) | Choose **Area** (fill a closed outline) or **Path** (fill a band along a drawn path). |
| **ALignment** (`AL`) | Path mode only: places the band at the **Center**, **Inside** or **Outside** of the path. |
| **Width** (`W`) | Path mode only: sets the band width. |
| **Arc** (`A`) | Switches to arc segments. In arc mode, **Line** (`L`) switches back to straight segments. |
| **Length** (`L`) | Straight segments only: adds a segment of a typed length. |
| **Close** (`C`) | Closes the outline once it has at least three points. |
| **Undo** (`U`) | Removes the last point. |
| **Exit** (Enter) | Finishes the drawn boundary. |

## Hatch settings panel
- **Type**: pattern, solid, gradient or user defined, with **Pattern**, **Angle**, **Scale**, **Spacing** and **Double** where they apply.
- **Color**, **Background**, **Color 1** and **Color 2** for gradients, **Layer** and **Transparency**.
- **Origin X** and **Origin Y** for the pattern origin.
- **Boundaries**: pick internal points, select objects or draw a boundary; **Keep boundaries**.
- **Islands**: Normal, Outer or Ignore.
- **Associative**, **Annotative**, **Create separate hatches**, **Gap tolerance** and **Draw order**.

## Notes
- If objects are selected when you start, the command starts in Select objects mode. If one of them is a hatch, the new hatch takes its settings.
- The command remembers the boundary method and draw mode you used last.
- Only objects visible on screen are used to find boundaries.
- [`HPDLGMODE`](HPDLGMODE-System-Variable) controls whether the settings panel opens by itself. [`HPASSOC`](HPASSOC-System-Variable) sets whether new hatches are associative, [`HPGAPTOL`](HPGAPTOL-System-Variable) sets the largest gap a boundary may bridge, and [`HPNAME`](HPNAME-System-Variable) holds the default pattern.
