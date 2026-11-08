---
summary: Sets which layer the text created by PDF text recognition goes on.
type: On/Off (0/1)
saved: Program settings
default: ON (1)
related: PDFSHXTEXT, PDFSHXBESTFONT, PDFSHXTEXTSETTINGS
---

## Values
| Value | Meaning |
|---|---|
| 0 (OFF) | The new text goes on the layer of the recognized geometry. |
| 1 (ON) | The new text goes on the current layer. |

## Notes
- You can type 0, 1, ON or OFF.
- The same choice is the Layer option of the [`PDFSHXTEXT`](PDFSHXTEXT-Command) settings (Current or Geometry).
