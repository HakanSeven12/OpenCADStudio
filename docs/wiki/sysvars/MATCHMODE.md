---
summary: Sets which properties MATCHPROP copies from the source object, one bit for each property.
type: Integer (bit-coded)
saved: Program settings
default: 261631 (every property except the plot style)
related: MATCHPROP
---

## Values
Add up the values of the properties you want copied.

| Value | Meaning |
|---|---|
| 0 | No property is copied. |
| 1 | Color |
| 2 | Layer |
| 4 | Linetype |
| 8 | Thickness |
| 16 | Linetype scale |
| 32 | Text |
| 64 | Dimension |
| 128 | Hatch |
| 256 | Lineweight |
| 512 | Plot style |
| 1024 | Polyline |
| 2048 | Viewport |
| 4096 | Table |
| 8192 | Material |
| 16384 | Kept as stored; it has no property of its own. |
| 32768 | Multileader |
| 65536 | Transparency |
| 131072 | Center marks and centerlines |

## Notes
- The settings dialog of [`MATCHPROP`](MATCHPROP-Command) changes the same value.
- Any whole number from 0 is accepted. A number larger than 32 bits keeps only its low 32 bits.
