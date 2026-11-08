---
summary: Sets the color that new objects get.
type: String
saved: Drawing
default: ByLayer
related: COLOR, CLAYER, CELTYPE, CETRANSPARENCY
---

## Values
| Value | Meaning |
|---|---|
| ByLayer | New objects take the color of their layer. |
| ByBlock | New objects take the color of the block they are inserted with. |
| 1-255 | A color index. |
| red, yellow, green, cyan, blue, magenta, white | Color names for indexes 1 to 7. |

## Notes
- Type `CECOLOR` or [`COLOR`](COLOR-Command) to be asked for a new color. Type `CECOLOR` with a value (for example `CECOLOR 1`) to set it at once.
- Pressing Enter without a value reports the current color.
- The color control on the ribbon shows and sets the same value.
