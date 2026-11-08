---
summary: Controls whether the frames of DWF underlays are shown and plotted.
type: Integer
saved: Drawing
default: 2 (Shown, not plotted)
related: FRAME, PDFFRAME, DGNFRAME, IMAGEFRAME, DWFOSNAP, DWFATTACH
---

## Values
| Value | Meaning |
|---|---|
| 0 | Frames are hidden and not plotted. |
| 1 | Frames are shown and plotted. |
| 2 | Frames are shown but not plotted. |

## Notes
- [`FRAME`](FRAME-System-Variable) sets this variable together with the other frame variables.
- Changing it redraws the DWF underlays in the drawing.
