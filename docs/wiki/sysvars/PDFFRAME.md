---
summary: Controls whether the frames of PDF underlays are shown and plotted.
type: Integer
saved: Drawing
default: 1 (shown and plotted)
related: FRAME, IMAGEFRAME, DWFFRAME, DGNFRAME, PDFATTACH
---

## Values
| Value | Meaning |
|---|---|
| 0 | PDF underlay frames are hidden and not plotted. |
| 1 | PDF underlay frames are shown and plotted. |
| 2 | PDF underlay frames are shown but not plotted. |

## Notes
- [`FRAME`](FRAME-System-Variable) sets all frame variables at once.
- Changing it can be undone.
