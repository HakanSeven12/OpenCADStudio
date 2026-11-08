---
summary: Sets the default outside diameter of new donuts.
type: Real
saved: Not saved
default: 1.0
related: DONUTID, DONUT
---

## Values
Any finite number larger than zero.

## Notes
- [`DONUT`](DONUT-Command) offers this value as its default outside diameter.
- When you draw a donut, the diameters you enter become the new defaults.
- If the inside diameter is larger than the outside diameter, the two are swapped.
- The value is kept only for the current session.
