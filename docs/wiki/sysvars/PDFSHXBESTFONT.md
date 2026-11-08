---
summary: Controls whether PDFSHXTEXT compares every font and keeps the one that matches best.
type: On/Off (0/1)
saved: Program settings
default: 0 (Off)
related: PDFSHXTEXT, PDFSHXLAYER, PDFSHXTEXTSETTINGS
---

## Values
| Value | Meaning |
|---|---|
| 0 | The first font that reaches the recognition threshold is used. |
| 1 | Every enabled font is compared and the best match is kept. |

## Notes
- Takes `1` / `0` or `ON` / `OFF`. The command line shows the value as `ON` or `OFF`.
- The settings of [`PDFSHXTEXT`](PDFSHXTEXT-Command) set the same option.
