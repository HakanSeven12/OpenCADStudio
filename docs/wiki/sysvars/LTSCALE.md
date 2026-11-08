---
summary: Sets the global linetype scale for all objects in the drawing.
type: Real
saved: Drawing
default: 1.0
related: CELTSCALE, PSLTSCALE, MSLTSCALE, LINETYPE
---

## Values
Any number greater than 0. Zero and negative values are refused. A larger value makes dashes and gaps longer.

## Notes
- Each object's own linetype scale ([`CELTSCALE`](CELTSCALE-System-Variable) for new objects) is multiplied by this value.
- The drawing redraws its linetypes as soon as the value changes.
- Changing it can be undone.
