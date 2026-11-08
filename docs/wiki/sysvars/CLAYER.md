---
summary: Sets the current layer, the layer that new objects are placed on.
type: String
saved: Drawing
default: 0
related: LAYER, CECOLOR, CELTYPE, CENTERLAYER
---

## Notes
- Type `CLAYER` with a layer name to make that layer current. The layer must already exist.
- Type `CLAYER` and press Enter without a name to see the current layer.
- [`SETVAR`](SETVAR-Command) can only read this variable. The [`LAYER`](LAYER-Command) manager and the layer control on the ribbon also set it.
