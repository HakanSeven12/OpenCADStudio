---
summary: Counts block references or other selected objects, highlights them in the drawing and flags overlapping duplicates.
related: COUNTLIST, COUNTAREA, COUNTFIELD, COUNTTABLE, COUNTCLOSE
---

## Usage
1. Specify the first corner of the count area, or type an option. Press Enter for **Current area**.
2. If you picked a corner, specify the opposite corner.
3. Select the target objects and press Enter. Press Enter with nothing selected, or type `L`, to list all blocks in the Count palette instead.
4. The command line shows the count, for example `Chair ...... 12`, and the drawing enters count mode.

## Options
| Option | What it does |
|---|---|
| **Current area** (`C`) | Counts in all of model space. |
| **Entire model space** (`E`) | Counts in all of model space. |
| **Object** (`O`) | Select a closed polyline as the count area. It must have only straight segments and must not cross itself. |
| **Polygonal** (`P`) | Pick the corners of a polygon. Press Enter after three or more points. |
| **List all blocks** (`L`) | Opens the Count palette with every block and its count, inside the area if you drew one. |

## Count mode
- The counted objects are highlighted, overlapping duplicates are marked as errors, and everything else is faded.
- A toolbar over the drawing shows the count and has buttons for **Previous** and **Next** reference, **Count in a specified area**, **Select objects**, **Insert Count Field** and **Close**.
- The count updates when you edit the drawing.
- Run [`COUNTCLOSE`](COUNTCLOSE-Command) or click **Close** to leave count mode.

## Notes
- If all targets are references of one block, that block is counted. Otherwise the selected objects are counted as a group.
- A rectangle or polygon area is drawn as a closed polyline on the layer `0-CountArea`. The layer is locked and does not plot.
- If you delete the area polyline while counting, a dialog asks whether to undo that change or go on counting all of model space. You can make it remember your choice.
- `COUNTCOLOR` and `COUNTERRORCOLOR` set the highlight colours (colour index 1 to 255). `COUNTNUMBER` reports the current count.
