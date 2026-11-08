---
summary: Closes the Block Editor, asking whether to save the changes first.
related: BEDIT, BSAVE, BSAVEAS, BTESTBLOCK
ribbon: Block Editor tab › Close panel
---

## Usage
1. Run the command while a block is open in the Block Editor.
2. If the block has unsaved changes, the **Block - Save Parameter Changes?** dialog asks what to do:
   - **Save the changes** saves the block definition and closes the editor.
   - **Discard the changes** throws the edits away and closes the editor.
   - **Cancel** keeps the Block Editor open.
3. If there are no changes, the editor closes right away.

## Notes
- The dialog shows how many references of the block are in the drawing, as saving can change them.
- When the editor closes, you return to the space and view you were in before you opened it.
- If no Block Editor is open, the command reports it and does nothing.
