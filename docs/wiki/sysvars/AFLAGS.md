---
summary: Sets the modes that new attribute definitions start with.
type: Integer (bit code)
saved: Not saved
default: 16 (Lock position)
related: ATTDEF, ATTDISP, ATTMODE
---

## Values
| Value | Meaning |
|---|---|
| 0 | No modes |
| 1 | Invisible |
| 2 | Constant |
| 4 | Verify |
| 8 | Preset |
| 16 | Lock position |
| 32 | Multiple lines |

The bits add up. For example, 17 means Invisible and Lock position. The accepted range is 0 to 63.

## Notes
- The [`ATTDEF`](ATTDEF-Command) dialog and its command-line form start with these modes.
- The modes you use in [`ATTDEF`](ATTDEF-Command) are kept for the next definition, except Multiple lines.
- The value is kept for the current session only. It goes back to 16 when the program starts.
