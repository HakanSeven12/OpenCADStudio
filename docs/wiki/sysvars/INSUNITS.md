---
summary: Sets the drawing units, used to scale blocks and drawings inserted from a drawing with other units.
type: Integer
saved: Drawing
default: 6 (Meters) for a new drawing; 0 (Unitless) for a file that does not store it
related: UNITS, DWGUNITS, LUNITS, CANNOSCALE
---

## Values
| Value | Meaning |
|---|---|
| 0 | Unitless |
| 1 | Inches |
| 2 | Feet |
| 3 | Miles |
| 4 | Millimeters |
| 5 | Centimeters |
| 6 | Meters |
| 7 | Kilometers |
| 8 | Microinches |
| 9 | Mils |
| 10 | Yards |
| 11 | Angstroms |
| 12 | Nanometers |
| 13 | Microns |
| 14 | Decimeters |
| 15 | Decameters |
| 16 | Hectometers |
| 17 | Gigameters |
| 18 | Astronomical units |
| 19 | Light years |
| 20 | Parsecs |
| 21 | US survey feet |
| 22 | US survey inches |
| 23 | US survey yards |
| 24 | US survey miles |

## Notes
- When you insert a block or drawing, it is scaled by the ratio of its units to this drawing's units. No scaling happens when either side is unitless.
- The units also choose between metric and imperial scale lists, and they affect the annotation scale.
- Setting this variable does not rescale existing objects. Use [`DWGUNITS`](DWGUNITS-Command) to convert the drawing.
