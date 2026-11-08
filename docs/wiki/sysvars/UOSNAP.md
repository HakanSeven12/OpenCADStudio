---
summary: Turns object snaps to the geometry inside PDF, DWF and DGN underlays on or off, all at once.
type: Integer
saved: Not saved
default: 1 (On)
related: PDFOSNAP, DWFOSNAP, DGNOSNAP, UOSNAP0, UOSNAP1
---

## Values
| Value | Meaning |
|---|---|
| 0 | Snaps do not find underlay geometry |
| 1 | Snaps find the geometry of every underlay kind |
| 2 | Read-only: the three underlay kinds have different settings |

## Notes
- Setting 0 or 1 sets [`PDFOSNAP`](PDFOSNAP-System-Variable), [`DWFOSNAP`](DWFOSNAP-System-Variable) and [`DGNOSNAP`](DGNOSNAP-System-Variable) together. You cannot set 2.
- The underlay snap list on the Insert ribbon tab shows and sets it.
- The value is not kept between sessions; it starts at 1.
