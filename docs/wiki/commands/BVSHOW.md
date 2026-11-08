---
summary: In the Block Editor, makes the selected objects visible in the current visibility state.
related: BVHIDE, BVSTATE, BEDIT
ribbon: Block Editor tab › Visibility panel
---

## Usage
1. Open a block in the Block Editor.
2. Select the objects to show, or run `BVSHOW` and select them at "Select objects to make visible:".
3. Press Enter. The objects become visible in the current visibility state.

## Notes
- Only objects that belong to the block being edited are changed.
- If the block has no visibility parameter yet, one is added at the block's base point, which makes the block dynamic.
- To see hidden objects so you can pick them, set [`BVMODE`](BVMODE-System-Variable) to 1 (show dimmed), or click **Visibility Mode** on the same panel.
- The ribbon button is labelled **Make Visible**.
