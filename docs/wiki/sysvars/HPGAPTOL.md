---
summary: Sets the largest gap a hatch boundary may bridge when you pick a point inside an area that is not fully closed.
type: Real
saved: Program settings
default: 0 (the boundary must be closed)
related: HATCH, HPNAME, HPASSOC
---

## Values
Any number from 0 to 5000, in drawing units. 0 means only closed areas are found. A larger value lets [`HATCH`](HATCH-Command) close gaps up to that size between objects.

## Notes
- You can also change it at the [`HATCH`](HATCH-Command) prompt; the new value is kept for later boundary picks.
- A value outside the range is refused.
