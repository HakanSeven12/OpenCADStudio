---
summary: Sets the layer that new hatches are placed on.
type: String
saved: Drawing
default: "." (use the current layer)
related: DIMLAYER, XREFLAYER, TEXTLAYER, MLEADERLAYER, HATCH
---

## Values
| Value | Meaning |
|---|---|
| . | New hatches go on the current layer. An empty value means the same. |
| layer name | New hatches go on this layer. If the layer does not exist, it is created when the first hatch is placed. |

## Notes
- If the value is not a valid layer name, new hatches go on the current layer.
- Type `HATCHLAYER` with no value to see the current setting and enter a new one.
