---
summary: Sets whether the clip boundaries of clipped blocks and external references are shown and plotted.
type: Integer
saved: Drawing
default: 2 (Shown, not plotted)
related: XCLIP, FRAME, WIPEOUTFRAME, IMAGEFRAME
---

## Values
| Value | Meaning |
|---|---|
| 0 | Clip boundaries are hidden and not plotted |
| 1 | Clip boundaries are shown and plotted |
| 2 | Clip boundaries are shown but not plotted |

## Notes
- Clip boundaries are made with [`XCLIP`](XCLIP-Command).
- [`FRAME`](FRAME-System-Variable) sets this and the other frame variables together.
