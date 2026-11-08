---
summary: Sets whether linetypes in layout viewports keep the same dash size on the sheet at every viewport scale.
type: On/Off (0/1)
saved: Drawing
default: 1 (On)
related: LTSCALE, MSLTSCALE, CELTSCALE
---

## Values
| Value | Meaning |
|---|---|
| 0 | Dashes in a viewport are scaled with the viewport, so they look smaller or larger on the sheet. |
| 1 | Dashes in a viewport are scaled by one over the viewport scale, so they keep their length on the sheet. |

## Notes
- Each layout keeps its own value.
- It also applies when a layout is plotted.
