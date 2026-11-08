---
summary: Changes the unit the drawing is measured in and scales model space so every object keeps its real size.
related: UNITS, SCALE
---

## Usage
1. Run `DWGUNITS`. The command line shows the current unit.
2. Choose the unit to convert to: Millimeters (`mm`), Centimeters (`cm`), Meters (`m`), Kilometers (`km`), Inches (`in`), Feet (`ft`), Miles (`mi`) or Yards (`yd`). Click a button or type the name or its short form.

The drawing's unit is changed, and everything in model space is scaled about the origin by the conversion factor. The command line reports the units, how many objects were scaled, the factor, and how many viewports were reframed.

## Notes
- You can also type the unit after the command, for example `DWGUNITS mm`.
- Layout viewports are reframed so they keep showing the same part of the model. The layouts themselves stay in paper units.
- Objects on locked layers are converted too, so the whole drawing stays at one scale.
- Length settings such as the linetype scale, the default text height and the starting radii for fillet and chamfer are converted with the drawing.
- Dimension styles keep their look: their overall scale is converted too.
- The active annotation scale is adjusted to match.
- If the drawing was unitless, only the unit label changes; nothing is scaled.
- Choosing the unit the drawing already uses does nothing.
- The **Convert drawing...** link in the status-bar units menu runs this command.
- To relabel the unit without scaling anything, use [`UNITS`](UNITS-Command).
