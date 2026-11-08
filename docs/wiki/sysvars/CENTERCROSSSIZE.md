---
summary: Sets the size of the center cross of new center marks.
type: String
saved: Drawing
default: 0.1x
related: CENTERCROSSGAP, CENTERMARKEXE, CENTEREXE, CENTERMARK, CENTERRESET
---

## Values
| Value | Meaning |
|---|---|
| A positive number | A fixed cross size in drawing units. |
| A positive number followed by `x` | A factor of the circle or arc diameter, for example `0.1x`. |
| ByLinetype | The size follows the default factor of 0.1 times the diameter. |

## Notes
- Used by [`CENTERMARK`](CENTERMARK-Command). [`CENTERRESET`](CENTERRESET-Command) applies the current value to existing center marks.
- A value that does not match these forms is refused and asked for again.
