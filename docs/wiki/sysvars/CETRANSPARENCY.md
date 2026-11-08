---
summary: Sets the transparency that new objects get.
type: String
saved: Drawing
default: ByLayer
related: CECOLOR, CLAYER, CELTYPE
---

## Values
| Value | Meaning |
|---|---|
| ByLayer (or -1) | New objects take the transparency of their layer. |
| ByBlock (or -2) | New objects take the transparency of the block they are inserted with. |
| 0-90 | A fixed transparency in percent. 0 is fully opaque. |

## Notes
- Type `CETRANSPARENCY` or use [`SETVAR`](SETVAR-Command). Other values are refused.
- The change can be undone.
