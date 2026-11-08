---
summary: Sets the layer that new external references go on.
type: String
saved: Drawing
default: . (Current layer)
related: XATTACH, XREF, HATCHLAYER
---

## Values
| Value | Meaning |
|---|---|
| . or empty | New references go on the current layer |
| A layer name | New references go on that layer; it is created if it does not exist |

## Notes
- A value that is not a valid layer name means the current layer.
- Type `XREFLAYER` alone to see the current value, or `XREFLAYER <name>` to set it.
