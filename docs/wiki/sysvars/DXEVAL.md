---
summary: Controls when data extraction tables are checked against the drawing, and whether outdated tables are updated without asking.
type: Integer (bit-coded)
saved: Drawing
default: 12 (Plot and Publish)
related: DATAEXTRACTION, DATALINKUPDATE, FIELDEVAL
---

## Values
Add the values of the bits you want. The accepted range is 0 to 511.

| Value | Meaning |
|---|---|
| 0 | Tables are never checked. |
| 1 | Check when the drawing opens. |
| 2 | Check when the drawing is saved. |
| 4 | Check when [`PLOT`](PLOT-Command) starts. |
| 8 | Check when publishing starts. |
| 16 | Check when the drawing is archived. |
| 32 | Update outdated tables without asking when saving. |
| 64 | Update outdated tables without asking when plotting. |
| 128 | Update outdated tables without asking when publishing. |
| 256 | Update outdated tables without asking when archiving. |

## Notes
- When a check finds outdated tables and the event does not update them by itself, a window asks you to update or skip each table.
- An open only asks; it never updates without asking.
- The archive bits (16 and 256) are kept in the value, but packaging a drawing does not check its tables yet.
- The plot check runs for the PLOT command, not for -PLOT.
