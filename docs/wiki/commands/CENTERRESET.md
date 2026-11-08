---
summary: Resets center marks and centerlines to the current center settings, undoing length changes made with grips.
related: CENTERMARK, CENTERLINE, CENTERDISASSOCIATE, CENTERREASSOCIATE
---

## Usage
1. Select the center marks or centerlines, or run `CENTERRESET` and select them at "Select center mark or centerline to reset:".
2. Press Enter. The command reports how many center objects were reset.

## Notes
- Centerlines get the extension from [`CENTEREXE`](CENTEREXE-System-Variable) at both ends again, and any lengthening or shortening is removed.
- Center marks get the cross size, gap and extension from [`CENTERCROSSSIZE`](CENTERCROSSSIZE-System-Variable), [`CENTERCROSSGAP`](CENTERCROSSGAP-System-Variable), [`CENTEREXE`](CENTEREXE-System-Variable) and [`CENTERMARKEXE`](CENTERMARKEXE-System-Variable), and any grip changes are removed.
- Only center marks and centerlines are taken from the selection; other objects are ignored.
