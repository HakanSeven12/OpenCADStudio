---
summary: Controls whether linetypes in model space are scaled by the current annotation scale.
type: On/Off (0/1)
saved: Drawing
default: 1 (On)
related: CANNOSCALE, LTSCALE, PSLTSCALE, CELTSCALE
---

## Values
| Value | Meaning |
|---|---|
| 0 | Model space linetypes ignore the annotation scale. |
| 1 | Model space linetypes are scaled by the annotation scale ([`CANNOSCALE`](CANNOSCALE-System-Variable)). |

## Notes
- A drawing that does not store the value uses 1.
