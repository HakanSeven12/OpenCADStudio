---
summary: In the Block Editor, makes the selected objects invisible in the current visibility state.
related: BVSHOW, BVSTATE, BEDIT
ribbon: Block Editor tab › Visibility panel
---

## Usage
1. Open a block in the Block Editor.
2. Select the objects to hide, or run `BVHIDE` and select them at "Select objects to hide:".
3. Press Enter. The objects are hidden in the current visibility state.

## Notes
- Only objects that belong to the block being edited are changed.
- If the block has no visibility parameter yet, one is added at the block's base point, which makes the block dynamic.
- Hidden objects are either left out of the editor view or shown dimmed, depending on [`BVMODE`](BVMODE-System-Variable). The **Visibility Mode** button on the same panel switches between the two.
- The ribbon button is labelled **Make Invisible**.
