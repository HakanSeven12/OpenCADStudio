---
summary: Sets the size of the pick box at the cursor and how close a click must be to select an object.
type: Integer
saved: Program settings
default: 3
related: APERTURE, PICKADD, PICKDRAG
---

## Values
Accepted values are whole numbers from 0 to 50. Larger numbers make a bigger box and a wider click area. 0 hides the box, but a click right on an object still selects it.

## Notes
- The number is a size step, not an exact pixel count.
- Set it in [`OPTIONS`](OPTIONS-Command) with the "Pick box size" slider in the Selection section.
