---
summary: Sets how many decimal places angles are shown with.
type: Integer
saved: Drawing
default: 0
related: AUNITS, ANGBASE, ANGDIR, UNITS
---

## Values
The accepted range is 0 to 8. A value outside the range is set to the nearest limit.

## Notes
- For degrees/minutes/seconds, 0 shows whole degrees, 1 to 2 adds minutes, 3 to 4 adds seconds, and higher values add decimals to the seconds.
- The [`UNITS`](UNITS-Command) dialog sets this value.
