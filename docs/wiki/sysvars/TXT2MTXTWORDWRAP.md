---
summary: Sets whether TXT2MTXT runs the combined lines on as one wrapped paragraph.
type: On/Off (0/1)
saved: Program settings
default: 1 (On)
related: TXT2MTXT, TXT2MTXTCOMBINEMTEXT, TXT2MTXTLINESPACING
---

## Values
| Value | Meaning |
|---|---|
| 0 | Each text object becomes its own paragraph |
| 1 | The lines are joined with spaces and wrap in a width taken from the selected objects |

## Notes
- Only used while [`TXT2MTXTCOMBINEMTEXT`](TXT2MTXTCOMBINEMTEXT-System-Variable) is 1.
- While this is 1, line spacing is always uniform.
- The Settings window of [`TXT2MTXT`](TXT2MTXT-Command) sets it too.
