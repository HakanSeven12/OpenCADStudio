---
summary: Controls whether object snaps find the geometry inside PDF underlays.
type: On/Off (0/1)
saved: Not saved
default: 1 (On)
related: UOSNAP, DWFOSNAP, DGNOSNAP, PDFATTACH
---

## Values
| Value | Meaning |
|---|---|
| 0 | Object snaps ignore the geometry of PDF underlays. |
| 1 | Object snaps find the geometry of PDF underlays. |

## Notes
- The Enable Snap button on the ribbon tab of a selected PDF underlay switches it.
- [`UOSNAP`](UOSNAP-System-Variable) sets it together with the DWF and DGN switches.
- The value lasts for the current session only.
