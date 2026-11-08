---
summary: Controls whether the frames of raster images are shown and plotted.
type: Integer
saved: Drawing
default: 1 (shown and plotted)
related: FRAME, PDFFRAME, DWFFRAME, DGNFRAME, WIPEOUTFRAME, XCLIPFRAME
---

## Values
| Value | Meaning |
|---|---|
| 0 | Image frames are hidden and not plotted. |
| 1 | Image frames are shown and plotted. |
| 2 | Image frames are shown but not plotted. |

## Notes
- The value is stored in the drawing. The last value you set is also kept in the program settings. A new drawing, or a drawing that does not store the value, uses that one.
- [`FRAME`](FRAME-System-Variable) sets all frame variables at once.
- Changing it can be undone.
