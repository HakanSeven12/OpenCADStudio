---
summary: Saves the changes to the block that is open in the Block Editor.
related: BSAVEAS, BCLOSE, BEDIT, BTESTBLOCK
ribbon: Block Editor tab › Open and Save panel
---

## Usage
1. With a block open in the Block Editor, run `BSAVE`.
2. The block definition is updated, and every reference to it in the drawing shows the new content. The editor stays open.

## Notes
- Works only while the Block Editor is open. Otherwise you see "No block editor is open."
- The save is refused if the block would contain itself through a nested block reference, or if its name is not a valid block name.
- For a dynamic block, the editor goes back to the first visibility state, and erased objects are dropped from the visibility states and actions.
- `BSAVE` saves the block, not the drawing file. Use [`SAVE`](SAVE-Command) to write the drawing. In the desktop app the drawing's recovery copy is updated at once, so a crash before you leave the editor keeps the saved block.
- `BSAVE` is refused in a Test Block window.
