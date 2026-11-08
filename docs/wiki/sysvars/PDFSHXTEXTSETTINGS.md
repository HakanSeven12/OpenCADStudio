---
summary: Makes the next PDF text recognition open its settings first.
type: On/Off (0/1)
saved: Program settings
default: 0 (Off)
related: PDFSHXTEXT, PDFSHXLAYER, PDFSHXBESTFONT
---

## Values
| Value | Meaning |
|---|---|
| 0 | [`PDFSHXTEXT`](PDFSHXTEXT-Command) asks for objects right away. |
| 1 | The next [`PDFSHXTEXT`](PDFSHXTEXT-Command) opens the recognition settings first. |

## Notes
- The setting works once: when the settings open, the value goes back to 0.
- You can type 0, 1, ON or OFF. The value is shown as a number.
