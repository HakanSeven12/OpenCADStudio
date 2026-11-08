---
summary: Turns Dynamic Input on or off.
type: Integer
saved: Program settings
default: 3 (On)
---

## Values
| Value | Meaning |
|---|---|
| 0 | Dynamic Input is off. |
| 1 | Dynamic Input is on (pointer input). |
| 2 | Dynamic Input is on (dimension input). |
| 3 | Dynamic Input is on (pointer and dimension input). |
| -1 to -3 | Dynamic Input is turned off for now. Turning it on again restores the matching positive value. |

## Notes
- The accepted range is -3 to 3.
- 1, 2 and 3 all turn Dynamic Input on. Open CAD Studio does not handle pointer input and dimension input separately yet.
- The Dynamic Input button on the status bar switches between the positive and negative value. If the value is 0, turning it on sets 3.
