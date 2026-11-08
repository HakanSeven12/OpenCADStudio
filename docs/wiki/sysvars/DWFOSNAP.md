---
summary: Controls whether object snaps find geometry inside attached DWF underlays.
type: On/Off (0/1)
saved: Not saved
default: 1 (On)
related: UOSNAP, PDFOSNAP, DGNOSNAP, DWFFRAME, DWFATTACH
---

## Values
| Value | Meaning |
|---|---|
| 0 | Object snaps ignore the geometry of DWF underlays. |
| 1 | Object snaps can find the geometry of DWF underlays. |

## Notes
- [`UOSNAP`](UOSNAP-System-Variable) sets the PDF, DWF and DGN snap variables at once.
- The value applies to all open drawings and is kept only for the current session.
