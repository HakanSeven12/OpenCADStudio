---
summary: Enlarges or reduces the view of the drawing in the current viewport.
related: PAN, ZS, 3DORBIT, VIEW
ribbon: View tab › Navigate panel
---

## Usage
1. Run `ZOOM`.
2. At `ZOOM WINDOW  Specify first corner:`, pick a corner of the area to zoom into, or type an option or a scale factor.
3. At `ZOOM WINDOW  Specify opposite corner:`, pick the other corner.

## Options
| Option | What it does |
|---|---|
| **Window** (`W`) | Zooms to a rectangle you pick by two corners (the default). |
| **Extents** (`E`) | Fits everything in the drawing into the view. |
| **Previous** (`P`) | Goes back to the previous view. |
| **Object** (`O`) | Zooms to the selected objects; with nothing selected it asks you to select them. |
| **All** (`A`) | In a plan view, shows the drawing limits or the extents, whichever is larger. In a 3D view it works like **Extents**. |
| **Dynamic** (`D`) | Drag horizontally to pan and vertically to zoom. Press Esc to stop. |
| **Extents All** (`EA`) | Zooms to extents in every model viewport. |
| **In** (`I`) / **Out** (`OUT`) | Zooms in or out by a fixed step (1.5 times). |
| **Scale** (`S`) | Asks for a scale factor. A number or `nX` (for example `2` or `0.5X`) scales the current view; `nXP` sets the scale of the active layout viewport relative to paper space. |

## Notes
- You can type a scale factor (`2`, `0.5X`, `1/50XP`) straight at the first prompt.
- `ZOOM` runs transparently inside another command when you type `'ZOOM`.
- The ribbon offers Zoom Extents, Zoom Window, Zoom In and Zoom Out.
