---
summary: Sets the format used to show and read angles.
type: Integer
saved: Drawing
default: 0 (Decimal degrees)
related: AUPREC, ANGBASE, ANGDIR, UNITS
---

## Values
| Value | Meaning |
|---|---|
| 0 | Decimal degrees |
| 1 | Degrees/minutes/seconds (for example 26d33'54") |
| 2 | Gradians (for example 50g) |
| 3 | Radians (for example 1.5708r) |
| 4 | Surveyor's units |

## Notes
- The [`UNITS`](UNITS-Command) dialog sets this value.
- A plain number you type for an angle is read in this format. Gradians and radians apply only for 2 and 3; other values read the number as degrees.
- [`SETVAR`](SETVAR-Command) accepts any whole number; values outside 0 to 4 show angles as decimal degrees.
