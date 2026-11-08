---
summary: Sets the layer that new multileaders are placed on.
type: String
saved: Drawing
default: . (the current layer)
related: MLEADER, DIMLAYER, TEXTLAYER
---

## Values
A layer name, or `.` for the current layer. An empty value also means the current layer.

## Notes
- If the named layer does not exist, it is created when the next multileader is made.
- The Layer option of [`MLEADER`](MLEADER-Command) and the layer box on the ribbon set the same value.
