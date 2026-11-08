---
summary: Sets how many isolines are drawn on each curved face of 3D solids and surfaces in wireframe display.
type: Integer
saved: Drawing
default: 8 for a new drawing; 4 for a file that does not store it
related: REGEN
---

## Values
Any whole number. Values are kept between 0 and 64; a larger value is stored as 64 and a negative one as 0.

## Notes
- The Options dialog has a slider for the same value.
- Existing solids show the new count after [`REGEN`](REGEN-Command).
