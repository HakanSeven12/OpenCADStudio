---
summary: Opens the Block Properties Table of the dynamic block that is open in the Block Editor.
related: BPARAMETER, BLOOKUPTABLE, BEDIT, BSAVE
---

## Usage
1. Open a block in the Block Editor that has at least one Linear or Polar parameter.
2. Run `BTABLE`. The Block Properties Table window opens.
3. Pick a parameter in **Choose input property** and click **Add Property** to add it as a column. **Remove** takes a column away.
4. Click **Add Row** to add a row, then type the values for each column. **Delete** removes a row.
5. Turn on **Block properties must match a row in the table** if references may only use the value sets in the table.
6. Click **OK** to keep the table, or **Cancel** to discard the changes.

## Notes
- If no block is open in the Block Editor, or the block has no Linear or Polar parameter, the command prints a message and stops.
- A table can have up to 1000 rows.
