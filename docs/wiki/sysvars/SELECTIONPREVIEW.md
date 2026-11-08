---
summary: Controls when objects light up as the cursor moves over them.
type: Integer (bit code)
saved: Program settings
default: 3 (both)
related: SELECTIONEFFECT, PICKBOX
---

## Values
| Value | Meaning |
|---|---|
| 0 | Off. |
| 1 | When no command is active. |
| 2 | During a command that asks you to select objects. |

Values add up: 3 turns on both.

## Notes
- The preview also covers a selection window while you drag it: the objects it would catch light up before you release.
- The two checkboxes in Options › Selection set the two bits.
