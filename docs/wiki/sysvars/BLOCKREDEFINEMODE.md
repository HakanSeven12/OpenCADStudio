---
summary: Sets whether a block taken from a palette replaces a block of the same name in the drawing.
type: Integer
saved: Program settings
default: 1
related: BLOCKSPALETTE, INSERT
---

## Values
| Value | Meaning |
|---|---|
| 0 | Keep the block already in the drawing |
| 1 | Keep the block already in the drawing |
| 2 | Redefine the block from its source |

The accepted range is 0 to 2.

## Notes
- It applies to blocks inserted from the Blocks palette and from tool palettes.
- With 2, a dynamic block cannot be redefined from a tool palette; the program says so and stops.
