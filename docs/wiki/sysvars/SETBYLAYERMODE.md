---
summary: Sets which properties SETBYLAYER resets to ByLayer.
type: Integer (bit code)
saved: Not saved
default: 255 (all)
related: SETBYLAYER
---

## Values
| Value | Meaning |
|---|---|
| 1 | Color |
| 2 | Linetype |
| 4 | Lineweight |
| 8 | Material |
| 16 | Plot style |
| 32 | Also change ByBlock values to ByLayer |
| 64 | Also change objects inside blocks |
| 128 | Transparency |

Values add up. Accepted range: 0 to 255.

## Notes
- The Settings option of [`SETBYLAYER`](SETBYLAYER-Command) toggles the property bits, and its two Yes/No questions set bits 32 and 64.
- The value resets to 255 when the program starts again.
