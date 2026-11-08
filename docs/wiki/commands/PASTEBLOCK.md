---
summary: Pastes the copied objects as one new block and places a reference to it at a point you pick.
related: PASTECLIP, PASTEORIG, COPYCLIP, BLOCK, INSERT, EXPLODE
ribbon: Draw tab › Clipboard panel
---

## Usage
1. Copy or cut objects with [`COPYCLIP`](COPYCLIP-Command), [`CUTCLIP`](CUTCLIP-Command) or [`COPYBASE`](COPYBASE-Command).
2. Start `PASTEBLOCK`. A new block definition is made from the copied objects.
3. Specify the insertion point for the block, or type an option. A preview follows the cursor.

## Options
| Option | What it does |
|---|---|
| **Scale** (`S`) | Sets the scale of the block reference before you place it. |
| **Rotate** (`R`) | Sets the rotation of the block reference before you place it. |

## Notes
- The block gets the next free name of the form `Block1`, `Block2`, and so on. Rename it with [`RENAME`](RENAME-Command) if you need to.
- The block's base point is the base point of the copied objects (their lower-left corner, or the point picked with `COPYBASE`).
- Blocks, multileader styles and clip settings used by the copied objects are brought along, so nested blocks keep their content.
- If the clipboard is empty, an error is shown and nothing changes.
