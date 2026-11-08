---
summary: Sets which properties, besides the object type, SELECTSIMILAR must match.
type: Integer (bit code)
saved: Program settings
default: 130 (layer and name)
related: SELECTSIMILAR
---

## Values
| Value | Meaning |
|---|---|
| 0 | Object type only. |
| 1 | Color |
| 2 | Layer |
| 4 | Linetype |
| 8 | Linetype scale |
| 16 | Lineweight |
| 32 | Plot style |
| 64 | Object style (text, dimension and other styles) |
| 128 | Name (for example the block name) |

Values add up. Accepted range: 0 to 255.

## Notes
- The Settings option of [`SELECTSIMILAR`](SELECTSIMILAR-Command) opens a dialog that sets the same bits.
