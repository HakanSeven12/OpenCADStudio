---
summary: Controls whether new dimensions are associative, plain dimension objects, or exploded into separate pieces.
type: Integer
saved: Drawing
default: 2 (Associative)
related: DIMASO, DIMLAYER, DIMREASSOCIATE, DIMDISASSOCIATE
---

## Values
| Value | Meaning |
|---|---|
| 0 | New dimensions are exploded at once into separate lines, arrowheads and text. |
| 1 | New dimensions are single dimension objects, but they do not follow the geometry they measure. |
| 2 | New dimensions are associative: they follow the geometry they measure when it moves or changes. |

## Notes
- It affects only dimensions you create after the change. Existing dimensions keep their state.
- Use [`DIMREASSOCIATE`](DIMREASSOCIATE-Command) and [`DIMDISASSOCIATE`](DIMDISASSOCIATE-Command) to change existing dimensions.
- SETVAR accepts any whole number; use 0, 1 or 2.
