---
summary: Sets the direction of angle 0, which other directions are counted from.
type: Real
saved: Drawing
default: 0 (East)
related: ANGDIR, AUNITS, AUPREC, UNITS
---

## Values
Any angle in degrees.

## Notes
- Directions you type, and directions the program shows, are measured from this angle. Angle sizes, such as how wide an arc opens, do not change.
- The [`UNITS`](UNITS-Command) dialog shows and sets this value in degrees.
- With [`SETVAR`](SETVAR-Command) or by typing the name, the value is typed and shown in degrees, for example 90 for North. Values outside 0–360 are brought into that range.
