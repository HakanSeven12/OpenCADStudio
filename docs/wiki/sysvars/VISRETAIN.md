---
summary: Sets whether your changes to external reference layers survive a reload.
type: On/Off (0/1)
saved: Drawing
default: 1 (On)
related: XREF, XATTACH
---

## Values
| Value | Meaning |
|---|---|
| 0 | Reloading a reference resets its layers to the values in the referenced file |
| 1 | Layer changes made in this drawing are kept when a reference reloads: on, frozen, locked, color, linetype, lineweight, plot style, plot and transparency |

## Notes
- Applies to the layers that come from a reference, named `reference|layer`.
- Reloading is done in [`XREF`](XREF-Command).
