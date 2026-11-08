---
summary: Opens the Block Editor Settings dialog for the colors, sizes and display options of parameters, actions and grips.
related: BEDIT, BACTIONBAR, BAUTHORPALETTE
ribbon: Block Editor tab › Manage panel
---

## Usage
1. Run the command in the Block Editor. The **Block Editor Settings** dialog opens.
2. Change the values you need.
3. Click **Apply** to save them, **Reset to Defaults** to restore the defaults, or **Cancel** to close without changes.

## Options
| Option | What it does |
|---|---|
| **Action color** | Color of action markers. |
| **Parameter color** | Color of parameters. |
| **Grip color** | Color of parameter grips. |
| **Grip size (pixels)** | Size of parameter grips on screen. |
| **Parameter text size (pixels)** | Size of parameter labels on screen. |
| **Parameter font** | Font of parameter labels. |
| **Keep parameter text horizontal** | Keeps parameter labels horizontal. |
| **Highlight dependent geometry** | Highlights the geometry that depends on the selected parameter or action. |
| **Show value-set markers** | Shows markers for parameters that have a value set. |
| **Dynamic grip color (index)** | Color of the grips on dynamic block references. |
| **Show grip tips** | Shows tips when you hover a grip. |
| **Display actions in bars** | Shows actions in action bars. |
| **Unconstrained / Partially constrained / Fully constrained / Over-constrained color** | Colors for geometry by constraint status. |

## Notes
- The command works only in the Block Editor.
- Colors take `BYLAYER`, `BYBLOCK`, an index from 1 to 255, or `RGB:r,g,b`. The constraint colors need an index or an RGB value.
- A font can be a font family, a font file, or `primary,bigfont`.
- The settings are saved with the program settings, not with the drawing.
