---
summary: Sets the layer that new dimensions are placed on.
type: String
saved: Drawing
default: "." (use the current layer)
related: HATCHLAYER, XREFLAYER, TEXTLAYER, MLEADERLAYER, DIMCONTINUEMODE
---

## Values
| Value | Meaning |
|---|---|
| . | New dimensions go on the current layer. An empty value means the same. |
| layer name | New dimensions go on this layer. If the layer does not exist, it is created when the first dimension is placed. |

## Notes
- Type `DIMLAYER` with no value to see the current setting and enter a new one.
- The Dimension Layer list on the ribbon and the Quick Access Toolbar shows and sets the same value.
