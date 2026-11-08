---
summary: Opens a temporary Test Block window where you can try the block that is open in the Block Editor.
related: BCLOSETESTBLOCK, BEDIT, BSAVE
ribbon: Block Editor tab › Open and Save panel
---

## Usage
1. With a block open in the Block Editor, run `BTESTBLOCK`.
2. A new tab named "Test Block Window - <block name>" opens. It holds one reference to the block, with its attributes, and the view zooms to fit it and its parameter grips.
3. Drag the grips of the reference to check how the block behaves.
4. Run [`BCLOSETESTBLOCK`](BCLOSETESTBLOCK-Command) or close the tab to go back to the Block Editor.

## Notes
- The test uses the block's current state in the editor, including changes you have not saved.
- The Test Block window is temporary. Saving, exporting, `WBLOCK`, `BEDIT`, `REFEDIT`, `BSAVE` and `BSAVEAS` are refused there.
- Running `BTESTBLOCK` again replaces the old test window with a new one.
