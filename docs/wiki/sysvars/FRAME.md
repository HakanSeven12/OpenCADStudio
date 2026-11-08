---
summary: Sets the frame display of images, underlays, wipeouts, clipped references and point cloud clips all at once.
type: Integer
saved: Drawing
default: 3 (Mixed) in a new drawing
related: IMAGEFRAME, PDFFRAME, DWFFRAME, DGNFRAME, WIPEOUTFRAME, XCLIPFRAME, POINTCLOUDCLIPFRAME, FRAMES0, FRAMES1, FRAMES2
---

## Values
| Value | Meaning |
|---|---|
| 0 | Frames are hidden and not plotted. |
| 1 | Frames are shown and plotted. |
| 2 | Frames are shown but not plotted. |
| 3 | Read only: the frame variables have different values. |

## Notes
- Setting 0, 1 or 2 sets [`IMAGEFRAME`](IMAGEFRAME-System-Variable), [`PDFFRAME`](PDFFRAME-System-Variable), [`DWFFRAME`](DWFFRAME-System-Variable), [`DGNFRAME`](DGNFRAME-System-Variable), [`WIPEOUTFRAME`](WIPEOUTFRAME-System-Variable), [`XCLIPFRAME`](XCLIPFRAME-System-Variable) and [`POINTCLOUDCLIPFRAME`](POINTCLOUDCLIPFRAME-System-Variable) to that value.
- You cannot set 3. A new drawing reads 3, because its frame variables start with different values.
- The Frames list on the Insert tab of the ribbon shows and sets this value.
- Changing the value can be undone.
