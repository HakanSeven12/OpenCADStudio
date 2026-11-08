---
summary: Controls whether the cursor is limited to horizontal and vertical directions.
type: On/Off (0/1)
saved: Drawing
default: 0 (Off)
related: ORTHO, OSMODE, SNAPANG
---

## Values
| Value | Meaning |
|---|---|
| 0 | Ortho mode is off. |
| 1 | Ortho mode is on: points you pick are kept horizontal or vertical from the last point. |

## Notes
- [`ORTHO`](ORTHO-Command), the F8 key and the status bar button switch it.
- Turning Ortho on turns polar tracking off.
- The value is saved with the drawing and restored when the drawing is opened.
