---
summary: Sets a drawing up for plotting: draws a sheet border in model space, or creates, deletes and scales viewports on a layout.
related: MVIEW, VPSCALE, PAGESETUP, LAYOUT
---

## Usage
In model space:
1. Enable paper space? [No/Yes] <Y>. Yes switches to the first layout and continues as on a layout.
2. With No: Enter the scale factor <1>.
3. Enter the paper width, then Enter the paper height. A closed rectangle of that size times the scale factor is drawn from 0,0, and the view zooms to it.

On a layout:
1. Enter an option [Create/Delete objects/Scale viewports].
2. Follow the prompts for the option you chose.

## Options
| Option | What it does |
|---|---|
| **Create** (`C`) | Enter viewport layout [Single/Engineering/Array] <Single>. Then specify first corner of bounding area (press Enter to use the whole sheet) and the opposite corner. |
| **Single** (`S`) | One viewport fills the bounding area. |
| **Engineering** (`E`) | Four viewports in a 2 by 2 grid: top, isometric, front and right views. You are asked for the X and Y distance between viewports. |
| **Array** (`A`) | Enter number of viewports in X direction and in Y direction (default 2), then the X and Y distance between them. |
| **Delete objects** (`D`) | Select the objects to delete on the layout. |
| **Scale viewports** (`S`) | Select the viewports to scale, then Enter the number of paper space units and Enter the number of model space units. The ratio sets the viewport scale. |

## Notes
- If the viewports do not fit in the bounding area with the gaps you gave, the command stops with a message.
