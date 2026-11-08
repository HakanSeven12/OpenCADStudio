---
summary: Copies properties such as layer, color, linetype, text, dimension and hatch settings from one source object to other objects.
related: LAYMCH, COPYTOLAYER, PROPERTIES
ribbon: Draw tab › Properties panel
---

## Usage
1. Select the source object. Press Enter instead to reuse the source from the last run, if it is still in the drawing.
2. The command line shows the current active settings, the properties that will be copied.
3. Select destination objects, or type an option. Hovering over an object previews the result.
4. Press Enter to finish.

## Options
| Option | What it does |
|---|---|
| **Settings** (`S`) | Opens the Property Settings window, where you turn each property on or off. |

## Notes
- If exactly one object is selected when you start, it becomes the source.
- The properties that can be copied are color, layer, linetype, linetype scale, lineweight, transparency, thickness, plot style, dimension, text, hatch, polyline, viewport, table, material, multileader and center object settings. The choice is stored in the `MATCHMODE` system variable.
- Objects on locked layers are not changed.
- You can apply the source to several selections in a row; the source stays highlighted until you finish.
