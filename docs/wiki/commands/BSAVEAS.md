---
summary: Saves the block that is open in the Block Editor as a new block definition with a new name.
related: BSAVE, BCLOSE, BEDIT
ribbon: Block Editor tab › Open and Save panel
---

## Usage
1. With a block open in the Block Editor, run `BSAVEAS`.
2. The Save Block As dialog opens. It lists the existing blocks and shows a preview of the block.
3. Type the new name in **Block name**.
4. Click **OK**.

## Notes
- The name must be a valid block name that is not used by another block. The dialog shows the problem if it is not.
- The original block keeps its last saved content. The editor carries on with the new block, so a later [`BSAVE`](BSAVE-Command) saves to the new name.
- Constraints and parameters of the block are copied to the new block.
- `BSAVEAS` is refused in a Test Block window.
