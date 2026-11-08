---
summary: Sets the gap between the center cross and the extension lines of new center marks.
type: String
saved: Drawing
default: 0.05x
related: CENTERCROSSSIZE, CENTERMARKEXE, CENTEREXE, CENTERMARK, CENTERRESET
---

## Values
| Value | Meaning |
|---|---|
| A positive number | A fixed gap length in drawing units. |
| A positive number followed by `x` | A factor of the circle or arc diameter, for example `0.05x`. The factor cannot be larger than 0.45. |
| ByLinetype | The gap follows the default factor of 0.05 times the diameter. |

## Notes
- Used by [`CENTERMARK`](CENTERMARK-Command). [`CENTERRESET`](CENTERRESET-Command) applies the current value to existing center marks.
- A value that does not match these forms is refused and asked for again.
