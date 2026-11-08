---
summary: Redefines the attribute definitions of a block from the attributes of one of its references, then updates every reference of the block.
related: ATTSYNC, ATTMAN, ATTDEF, BLOCK
---

## Usage
1. Enter name of block to redefine.

You can also type the name on the same line, for example `ATTREDEF TITLEBLOCK`.

The command takes the attributes of one reference of the block as the new attribute definitions. A selected reference of the block is used if there is one; otherwise the first reference in the drawing is used. The old attribute definitions of the block are replaced, and all references of the block are then brought in line with the new definitions.

## Notes
- Each new definition keeps the position, text settings and modes the attribute has on the reference, converted back into the block's own coordinates. The prompt is taken from the old definition with the same tag, or is the tag itself when there is none.
- The command line reports how many attributes the block now has.
- An error is shown if there is no block with that name, no reference of it, or the reference has no attributes.
- References on locked layers are not updated.
- The whole change is one undo step.
