---
summary: Starts the drawing command for the type of the selected object, using that object's layer, color, linetype and other general properties.
related: MATCHPROP, SELECTSIMILAR
---

## Usage
1. Select an object to use as a template.
2. Start the command. The matching drawing command starts, for example [`LINE`](LINE-Command) for a line or [`CIRCLE`](CIRCLE-Command) for a circle.
3. Draw the new object as usual.

## Notes
- If several objects are selected, the first one is the template.
- The new object takes the template's layer, color, transparency, linetype, linetype scale and lineweight. A dimension template also sets its dimension style.
- These settings are used only for the new object. The previous current layer, color and other defaults come back when the drawing command ends.
- Supported types: point, line, circle, arc, ellipse, polylines (all start [`PLINE`](PLINE-Command)), text, multiline text, spline, hatch, 2D solid, ray, construction line and dimensions (each starts its own dimension command).
- For other types the command reports that creating that object is not supported.
- With nothing selected, the command only prints a hint.
