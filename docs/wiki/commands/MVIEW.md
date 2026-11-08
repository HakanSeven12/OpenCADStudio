---
summary: Creates layout viewports on a paper space sheet: rectangular, polygonal, from an object, fitted to the sheet, or from a named view.
related: VPCLIP, MVSETUP, VPORTS, MSPACE, PSPACE, VPSCALE
ribbon: Layout tab › Viewport panel
---

## Usage
1. Switch to a layout tab. In model space the command stops with a message.
2. Specify corner of viewport, or type an option.
3. Specify opposite corner. The rectangular viewport is created.

## Options
| Option | What it does |
|---|---|
| **Polygonal** (`P`) | Draws the viewport outline point by point. Specify start point, then next points. Inside it, **Arc** (`A`) and **Line** (`L`) switch between straight and arc segments, **Close** (`C`) finishes the outline (three points or more), **Undo** (`U`) removes the last point. |
| **Object** (`O`) | Select a circle, full ellipse or closed polyline. The viewport takes its shape. |
| **Fit** (`F`) | Creates one viewport that fills the printable area of the sheet. |
| **Insert view** (`I`) | Choose a named view, or **New** (`N`) to define one by picking two model-space corners. Then specify the placement point on the sheet. |

## Notes
- The viewport boundary follows the printable area of the layout's page setup.
- Use [`VPCLIP`](VPCLIP-Command) to clip an existing viewport with a polygon or an object.
