---
summary: Sets the format used to show lengths and coordinates.
type: Integer
saved: Drawing
default: 2 (Decimal)
related: LUPREC, AUNITS, AUPREC, UNITS, INSUNITS
---

## Values
| Value | Meaning |
|---|---|
| 1 | Scientific |
| 2 | Decimal |
| 3 | Engineering (feet and decimal inches) |
| 4 | Architectural (feet and fractional inches) |
| 5 | Fractional |

## Notes
- [`UNITS`](UNITS-Command) sets it from a dialog.
- Architectural and fractional lengths are shown to the nearest 1/64 of a unit. Other values are shown as decimal numbers.
- Changing it can be undone.
