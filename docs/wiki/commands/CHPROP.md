---
summary: Changes the layer, colour, linetype, linetype scale or transparency of selected objects at the command line.
related: PROPERTIES, MATCHPROP, SETBYLAYER
---

## Usage
1. Select the objects, or run `CHPROP` and select them, then press Enter.
2. At "CHPROP  property  [Layer / Color / Linetype / LtScale / Transparency]:", type a property.
3. Type the new value and press Enter. The command reports how many objects were updated.

## Options
| Option | What it does |
|---|---|
| **Layer** (`LAYER`) | Moves the objects to the layer you name. |
| **Color** (`COLOR`) | Sets the colour as a colour number: 1–255, 256 for ByLayer, 0 for ByBlock. |
| **Linetype** (`LINETYPE`) | Sets the linetype you name. |
| **LtScale** (`LTSCALE`) | Sets the linetype scale of the objects. |
| **Transparency** (`TRANSPARENCY`) | Sets the transparency, from 0 to 90 percent. |

## Command-line form
You can give the property and value on one line, for example `CHPROP COLOR 1` or `CHPROP LAYER Walls`. This works on the objects that are selected and suits scripts.

## Notes
- Objects on locked layers are not changed.
- An invalid value, such as a colour name at the Color prompt, prints an error and changes nothing.
- The change can be undone with [`UNDO`](UNDO-Command).
