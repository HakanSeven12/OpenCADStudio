---
summary: Controls whether object snaps find the geometry inside attached DGN underlays.
type: On/Off (0/1)
saved: Not saved
default: 1 (On)
related: UOSNAP, PDFOSNAP, DWFOSNAP, DGNATTACH
---

## Values
| Value | Meaning |
|---|---|
| 0 | Object snaps ignore DGN underlay geometry. |
| 1 | Object snaps find DGN underlay geometry. |

## Notes
- [`UOSNAP`](UOSNAP-System-Variable) sets this together with the PDF and DWF switches.
- The Enable Snap button of the underlay ribbon tab toggles it for the selected underlay type.
- It returns to 1 when the program starts.
