---
summary: Command that sets how block attributes are displayed in the drawing.
type: Command (sets ATTMODE)
saved: Drawing
default: Normal
related: ATTMODE, ATTMODE0, ATTMODE1, ATTMODE2
---

## Values
| Value | Meaning |
|---|---|
| Normal | Visible attributes are shown; invisible ones stay hidden |
| ON | All attributes are shown, including invisible ones |
| OFF | No attributes are shown |

## Notes
- The command asks "Enter attribute visibility setting [Normal/ON/OFF]". You can also type it in one line, for example `ATTDISP ON`.
- It stores the choice in [`ATTMODE`](ATTMODE-System-Variable) as 1 (Normal), 2 (ON) or 0 (OFF) and redraws the drawing.
- The attribute definitions themselves are not changed.
