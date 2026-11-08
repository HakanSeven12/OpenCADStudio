---
summary: In the Block Editor, turns geometry into construction geometry or back, and shows or hides all construction geometry.
related: BEDIT, BCLOSE
ribbon: Block Editor tab › Manage panel
---

## Usage
1. Select the geometry and press Enter, or type an option.
2. With geometry selected, choose **Convert** or **Revert**. Press Enter to convert.

If you select geometry before you start the command, the selection is used and you go straight to step 2.

## Options
| Option | What it does |
|---|---|
| **Convert** (`C`) | Turns the selected geometry into construction geometry. |
| **Revert** (`R`) | Turns the selected construction geometry back into normal geometry. |
| **Show All** (`S`) | Shows all construction geometry in the Block Editor. |
| **Hide All** (`H`) | Hides all construction geometry in the Block Editor. |

## Notes
- The command works only in the Block Editor.
- Construction geometry is drawn grey and dashed in the Block Editor. It stays invisible in the block references, so you can use it as a helper while you build the block.
- **Show All** and **Hide All** change only the display in the editor; they do not change the block.
- After a convert or revert, the command reports how many objects changed. The change can be undone.
