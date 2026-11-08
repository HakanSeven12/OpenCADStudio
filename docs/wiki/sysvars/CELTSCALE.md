---
summary: Sets the linetype scale that new objects get.
type: Real
saved: Drawing
default: 1.0
related: LTSCALE, CELTYPE, LINETYPE
---

## Values
Any positive number. Zero and negative values are refused.

## Notes
- The final dash size of an object is its own linetype scale multiplied by [`LTSCALE`](LTSCALE-System-Variable).
- Set it by typing `CELTSCALE`, or with [`SETVAR`](SETVAR-Command).
