---
summary: Sets the shape a new revision cloud starts from in REVCLOUD.
type: Integer
saved: Program settings
default: 1 (Rectangular)
related: REVCLOUD, REVCLOUDGRIPS
---

## Values
| Value | Meaning |
|---|---|
| 0 | Freehand |
| 1 | Rectangular |
| 2 | Polygonal |

## Notes
- Choosing Rectangular, Polygonal or Freehand inside [`REVCLOUD`](REVCLOUD-Command) also changes this variable, so the next cloud starts the same way.
- Any other value is refused and you are asked again.
