---
summary: Sets the layer that new text and multiline text are placed on.
type: String
saved: Drawing
default: "" (empty: the current layer)
related: TEXT, MTEXT, CLAYER
---

## Values
Type a layer name to use it. Type `.` to go back to the current layer. Names with characters that a layer name cannot have are refused.

## Notes
- Objects made with [`TEXT`](TEXT-Command) and [`MTEXT`](MTEXT-Command) go on this layer. If the layer does not exist, it is created.
