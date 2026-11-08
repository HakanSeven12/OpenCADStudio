---
summary: Sets whether plots run in the background while you keep working.
type: Integer (bit code)
saved: Program settings
default: 1 (Single plots in the background)
related: PLOT, PRINTALL, PUBLISH
---

## Values
| Value | Meaning |
|---|---|
| 0 | No plots run in the background |
| 1 | Single plots run in the background |
| 2 | Batch plots run in the background |

The bits add up. 3 means both. The accepted range is 0 to 3.

## Notes
- Bit 1 is the "Plot in background" box of the [`PLOT`](PLOT-Command) dialog.
- Batch plots are the PDF output of [`PRINTALL`](PRINTALL-Command) and [`PUBLISH`](PUBLISH-Command).
